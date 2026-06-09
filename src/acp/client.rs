//! ACP client implementation for Writ.
//!
//! Uses GPUI's EventEmitter pattern - the handler emits events via AsyncApp,
//! and AgentView subscribes to receive them.

use agent_client_protocol::{
    Agent, CancelNotification, Client, ClientCapabilities, ClientSideConnection, ContentBlock,
    FileSystemCapability, Implementation, InitializeRequest, NewSessionRequest,
    PermissionOptionKind, PromptRequest, ReadTextFileRequest, ReadTextFileResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse, ResourceLink,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, TextContent,
    WriteTextFileRequest, WriteTextFileResponse,
};
use async_process::{Command, Stdio};
use gpui::{App, AppContext, AsyncApp, Context, Entity, EventEmitter, Task, WeakEntity};
use smol::channel;
use std::path::PathBuf;

use crate::editor::Editor;

/// Events emitted by AcpClient that subscribers can handle.
#[derive(Debug, Clone)]
pub enum AcpEvent {
    Ready,
    AgentMessageChunk(String),
    ResponseComplete,
    WritePending,
    WriteFile { path: PathBuf, content: String },
    Error(String),
}

/// Handler that receives ACP callbacks and emits events via AsyncApp.
struct ClientHandler {
    /// Async app context for accessing GPUI state.
    async_app: AsyncApp,
    /// The document editor to read from.
    document_editor: Entity<Editor>,
    /// Weak reference to the AcpClient entity for emitting events.
    acp_client: WeakEntity<AcpClient>,
}

impl ClientHandler {
    /// Emit an event to subscribers.
    fn emit(&self, event: AcpEvent) {
        if let Some(client) = self.acp_client.upgrade() {
            let _ = self.async_app.clone().update_entity(&client, |_, cx| {
                cx.emit(event);
            });
        }
    }

    /// Read file content, preferring the editor buffer for the current document.
    fn read_file_content(
        &self,
        path: &PathBuf,
        line: Option<u32>,
        limit: Option<u32>,
    ) -> Result<String, String> {
        // Try to read from editor buffer if it's our file
        let result =
            self.async_app
                .read_entity(&self.document_editor, |editor: &Editor, _: &App| {
                    let document_path = editor.file_path();

                    let is_our_file = document_path
                        .map(|doc_path| {
                            let doc_canonical = std::fs::canonicalize(doc_path).ok();
                            let change_canonical = std::fs::canonicalize(path).ok();
                            match (doc_canonical, change_canonical) {
                                (Some(a), Some(b)) => a == b,
                                _ => doc_path == path,
                            }
                        })
                        .unwrap_or(false);

                    if is_our_file {
                        Some(editor.text())
                    } else {
                        None
                    }
                });

        let content = match result {
            Ok(Some(text)) => text,
            Ok(None) | Err(_) => {
                // Fall back to disk
                std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?
            }
        };

        Ok(Self::apply_line_limit(&content, line, limit))
    }

    /// Apply line offset and limit to content (line-based, 1-indexed).
    fn apply_line_limit(content: &str, line: Option<u32>, limit: Option<u32>) -> String {
        let start_line = line.unwrap_or(1) as usize;
        let max_lines = limit.map(|l| l as usize).unwrap_or(usize::MAX);

        content
            .lines()
            .skip(start_line.saturating_sub(1))
            .take(max_lines)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[async_trait::async_trait(?Send)]
impl Client for ClientHandler {
    async fn request_permission(
        &self,
        request: RequestPermissionRequest,
    ) -> agent_client_protocol::Result<RequestPermissionResponse> {
        // Auto-select the first "allow once" or "allow always" option
        let option = request
            .options
            .iter()
            .find(|o| {
                matches!(
                    o.kind,
                    PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
                )
            })
            .or_else(|| request.options.first());

        match option {
            Some(opt) => Ok(RequestPermissionResponse::new(
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                    opt.option_id.clone(),
                )),
            )),
            None => Ok(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            )),
        }
    }

    async fn session_notification(
        &self,
        notification: SessionNotification,
    ) -> agent_client_protocol::Result<()> {
        match notification.update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let ContentBlock::Text(TextContent { text, .. }) = chunk.content {
                    self.emit(AcpEvent::AgentMessageChunk(text));
                }
            }
            SessionUpdate::ToolCall(_) => {
                // ToolCall notifications are informational only.
                // The actual file operations come through read/write_text_file RPCs.
            }
            _ => {
                // Ignore other updates (tool call updates, thoughts, etc.)
            }
        }
        Ok(())
    }

    async fn read_text_file(
        &self,
        request: ReadTextFileRequest,
    ) -> agent_client_protocol::Result<ReadTextFileResponse> {
        match self.read_file_content(&request.path, request.line, request.limit) {
            Ok(content) => Ok(ReadTextFileResponse::new(content)),
            Err(e) => Err(agent_client_protocol::Error::new(-32000, e)),
        }
    }

    async fn write_text_file(
        &self,
        request: WriteTextFileRequest,
    ) -> agent_client_protocol::Result<WriteTextFileResponse> {
        // Emit WritePending first to block file watcher reloads
        self.emit(AcpEvent::WritePending);
        // Then emit the actual write
        self.emit(AcpEvent::WriteFile {
            path: request.path.clone(),
            content: request.content.clone(),
        });
        Ok(WriteTextFileResponse::new())
    }
}

/// Commands sent to the ACP connection task.
enum AcpCommand {
    SendPrompt {
        text: String,
        file_path: Option<PathBuf>,
    },
    Cancel,
}

/// ACP client that manages the connection to an agent.
///
/// Implements `EventEmitter<AcpEvent>` - use `cx.subscribe()` to receive events.
pub struct AcpClient {
    /// Channel to send commands to the connection task.
    command_tx: channel::Sender<AcpCommand>,
    /// Handle to the connection task (keeps it alive).
    #[allow(dead_code)]
    task: Task<()>,
}

impl EventEmitter<AcpEvent> for AcpClient {}

impl AcpClient {
    /// Spawn an agent subprocess and establish an ACP connection.
    pub fn new(
        agent_command: String,
        cwd: PathBuf,
        document_editor: Entity<Editor>,
        cx: &mut Context<Self>,
    ) -> Self {
        let (command_tx, command_rx) = channel::unbounded::<AcpCommand>();
        let foreground = cx.foreground_executor().clone();
        let async_app = cx.to_async();
        let weak_self = cx.weak_entity();

        // Spawn the main connection task
        let task = cx.spawn(async move |_, _cx| {
            if let Err(e) = run_connection(
                agent_command,
                cwd,
                document_editor,
                async_app.clone(),
                weak_self.clone(),
                command_rx,
                foreground,
            )
            .await
            {
                // Emit error event
                emit_event(&async_app, &weak_self, AcpEvent::Error(e.to_string()));
            }
        });

        Self { command_tx, task }
    }

    /// Send a prompt to the agent.
    pub fn send_prompt(&self, text: String, file_path: Option<PathBuf>) {
        let _ = self
            .command_tx
            .try_send(AcpCommand::SendPrompt { text, file_path });
    }

    /// Cancel the current prompt.
    pub fn cancel(&self) {
        let _ = self.command_tx.try_send(AcpCommand::Cancel);
    }
}

/// Helper to emit an event from async context.
fn emit_event(async_app: &AsyncApp, acp_client: &WeakEntity<AcpClient>, event: AcpEvent) {
    if let Some(client) = acp_client.upgrade() {
        let _ = async_app.clone().update_entity(&client, |_, cx| {
            cx.emit(event);
        });
    }
}

async fn run_connection(
    agent_command: String,
    cwd: PathBuf,
    document_editor: Entity<Editor>,
    async_app: AsyncApp,
    acp_client: WeakEntity<AcpClient>,
    command_rx: channel::Receiver<AcpCommand>,
    foreground: gpui::ForegroundExecutor,
) -> anyhow::Result<()> {
    // Spawn the agent subprocess
    let mut child = Command::new(&agent_command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;

    let stdin = child.stdin.take().expect("stdin should be available");
    let stdout = child.stdout.take().expect("stdout should be available");

    let handler = ClientHandler {
        async_app: async_app.clone(),
        document_editor,
        acp_client: acp_client.clone(),
    };

    // Create the ACP connection
    let (conn, io_future) = ClientSideConnection::new(handler, stdin, stdout, move |fut| {
        foreground.spawn(fut).detach();
    });

    // Run the I/O handler in the background
    smol::spawn(async move {
        if let Err(e) = io_future.await {
            eprintln!("ACP I/O error: {}", e);
        }
    })
    .detach();

    // Initialize the connection
    let session_id = {
        use agent_client_protocol::ProtocolVersion;

        let fs_caps = FileSystemCapability::new()
            .read_text_file(true)
            .write_text_file(true);
        let client_caps = ClientCapabilities::new().fs(fs_caps);

        let request = InitializeRequest::new(ProtocolVersion::LATEST)
            .client_info(Implementation::new(
                "writ".to_string(),
                env!("CARGO_PKG_VERSION").to_string(),
            ))
            .client_capabilities(client_caps);

        conn.initialize(request).await?;

        // Create a session
        let request = NewSessionRequest::new(cwd);
        let response = conn.new_session(request).await?;
        response.session_id
    };

    // Signal that we're ready
    emit_event(&async_app, &acp_client, AcpEvent::Ready);

    // Process commands
    while let Ok(cmd) = command_rx.recv().await {
        match cmd {
            AcpCommand::SendPrompt { text, file_path } => {
                // Build prompt content blocks
                let mut content_blocks: Vec<ContentBlock> = Vec::new();

                if let Some(ref path) = file_path {
                    let uri = format!("file://{}", path.display());
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "file".to_string());
                    content_blocks.push(ContentBlock::ResourceLink(ResourceLink::new(name, uri)));
                }

                content_blocks.push(ContentBlock::Text(TextContent::new(text)));

                let request = PromptRequest::new(session_id.clone(), content_blocks);

                match conn.prompt(request).await {
                    Ok(_) => {
                        emit_event(&async_app, &acp_client, AcpEvent::ResponseComplete);
                    }
                    Err(e) => {
                        emit_event(&async_app, &acp_client, AcpEvent::Error(e.to_string()));
                    }
                }
            }
            AcpCommand::Cancel => {
                let _ = conn
                    .cancel(CancelNotification::new(session_id.clone()))
                    .await;
            }
        }
    }

    Ok(())
}
