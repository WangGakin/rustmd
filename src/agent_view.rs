//! Agent view component with split-pane layout.
//!
//! Provides a horizontal split between the document editor (left) and
//! an optional chat panel (right) for agent responses.

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, Rems, Render, Subscription, Window,
    actions, div, prelude::*, px, rgb,
};
use std::path::PathBuf;

use crate::acp::{AcpClient, AcpEvent};
use crate::diff::DiffState;
use crate::editor::{Editor, EditorConfig};

actions!(agent_view, [ToggleChatPanel, SubmitPrompt]);

/// Container managing the document editor and optional chat panel.
pub struct AgentView {
    /// The main document editor (left pane).
    document_editor: Entity<Editor>,
    /// The chat panel editor (right pane), read-only.
    chat_editor: Entity<Editor>,
    /// The prompt input editor (bottom of chat pane).
    prompt_editor: Entity<Editor>,
    /// Whether the chat panel is currently visible.
    chat_panel_visible: bool,
    /// Focus handle for the agent view.
    focus_handle: FocusHandle,
    /// Optional ACP client for communicating with an agent.
    acp_client: Option<Entity<AcpClient>>,
    /// Subscription to ACP client events.
    #[allow(dead_code)]
    acp_subscription: Option<Subscription>,
    /// Whether we're currently waiting for an agent response.
    awaiting_response: bool,
}

impl AgentView {
    /// Create a new agent view with the given document content and editor config.
    pub fn new(document_content: &str, config: EditorConfig, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();

        // Create the main document editor
        let document_editor =
            cx.new(|cx| Editor::with_config(document_content, config.clone(), cx));

        // Create the chat editor - starts empty, will be filled by agent responses
        let chat_editor = cx.new(|cx| {
            let mut editor = Editor::with_config("", config.clone(), cx);
            // Block input to make it read-only
            editor.set_input_blocked(true);
            // Not primary - don't update global status bar/title bar
            editor.set_primary(false);
            editor
        });

        // Create the prompt input editor with compact padding and no max width
        let prompt_config = EditorConfig {
            padding_x: Rems(1.0),
            padding_top: Rems(1.0),
            padding_bottom: Rems(1.0),
            max_line_width: None,
            ..config
        };
        let prompt_editor = cx.new(|cx| {
            let mut editor = Editor::with_config("", prompt_config, cx);
            // Not primary - don't update global status bar/title bar
            editor.set_primary(false);
            editor
        });

        Self {
            document_editor,
            chat_editor,
            prompt_editor,
            chat_panel_visible: false,
            focus_handle,
            acp_client: None,
            acp_subscription: None,
            awaiting_response: false,
        }
    }

    /// Connect to an agent using the given command.
    pub fn connect_agent(&mut self, agent_command: String, cwd: PathBuf, cx: &mut Context<Self>) {
        // Create the ACP client, passing the document editor for direct buffer access
        let document_editor = self.document_editor.clone();
        let client = cx.new(|cx| AcpClient::new(agent_command, cwd, document_editor, cx));

        // Subscribe to client events
        let subscription = cx.subscribe(&client, |this, _client, event, cx| {
            this.handle_acp_event(event, cx);
        });

        self.acp_client = Some(client);
        self.acp_subscription = Some(subscription);
    }

    /// Handle an event from the ACP client.
    fn handle_acp_event(&mut self, event: &AcpEvent, cx: &mut Context<Self>) {
        match event {
            AcpEvent::Ready => {
                // Agent connected, update chat with welcome message
                self.chat_editor.update(cx, |editor, cx| {
                    editor.set_text("*Agent connected. Type a message below.*\n\n", cx);
                });
            }
            AcpEvent::AgentMessageChunk(text) => {
                // Start streaming mode on first chunk
                if !self.awaiting_response {
                    self.awaiting_response = true;
                    self.chat_editor.update(cx, |editor, cx| {
                        editor.begin_streaming(cx);
                    });
                }
                // Append the chunk
                self.chat_editor.update(cx, |editor, cx| {
                    editor.append(text, cx);
                });
            }
            AcpEvent::ResponseComplete => {
                self.awaiting_response = false;
                self.chat_editor.update(cx, |editor, cx| {
                    editor.end_streaming(cx);
                    // Ensure consistent spacing after response
                    let ends_n = editor.ends_with("\n");
                    let ends_nn = editor.ends_with("\n\n");
                    if !ends_n {
                        editor.append("\n\n", cx);
                    } else if !ends_nn {
                        editor.append("\n", cx);
                    }
                });
            }
            AcpEvent::WritePending => {
                // Block file watcher reloads until WriteFile is processed
                self.document_editor.update(cx, |editor, _cx| {
                    editor.set_pending_agent_write(true);
                });
            }
            AcpEvent::WriteFile { path, content } => {
                self.apply_agent_content_change(path, content, cx);
            }
            AcpEvent::Error(msg) => {
                self.chat_editor.update(cx, |editor, cx| {
                    editor.append(&format!("\n\n**Error:** {}\n\n", msg), cx);
                });
            }
        }
    }

    /// Apply a content change from the agent (edit or write).
    /// Returns true if the change was applied, false if it was for a different file.
    fn apply_agent_content_change(
        &mut self,
        path: &PathBuf,
        content: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let document_path = self.document_editor.read(cx).file_path().cloned();

        let is_our_file = document_path
            .as_ref()
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
            self.document_editor.update(cx, |editor, cx| {
                let old_snapshot = editor.render_snapshot();
                let old_text = editor.text();

                editor.set_text(content, cx);

                let diff_state = DiffState::compute(old_snapshot, &old_text, content);
                editor.set_diff_state(Some(diff_state));
                editor.set_pending_agent_write(false);
            });
            true
        } else {
            eprintln!(
                "[ACP] Change for different file (doc: {:?}, change: {:?})",
                document_path, path
            );
            false
        }
    }

    /// Submit the current prompt to the agent.
    pub fn submit_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(client) = &self.acp_client else {
            return;
        };

        // Get the prompt text
        let prompt = self.prompt_editor.read(cx).text();
        if prompt.trim().is_empty() {
            return;
        }

        // If we're still waiting for a response, cancel it first
        if self.awaiting_response {
            client.read(cx).cancel();
            self.chat_editor.update(cx, |editor, cx| {
                editor.end_streaming(cx);
                editor.append("\n\n*Cancelled*\n\n", cx);
            });
        }

        // Get the file path from the document editor
        let file_path = self.document_editor.read(cx).file_path().cloned();

        // Clear the prompt editor
        self.prompt_editor.update(cx, |editor, cx| {
            editor.set_text("", cx);
        });

        // Add the user message to the chat (with background highlighting)
        self.chat_editor.update(cx, |editor, cx| {
            editor.append_user_message(&format!("{}\n\n", prompt.trim()), cx);
        });

        // Mark as awaiting response before sending
        self.awaiting_response = true;

        // Send to the agent with file context
        client.read(cx).send_prompt(prompt, file_path);
    }

    /// Get a reference to the document editor.
    pub fn document_editor(&self) -> &Entity<Editor> {
        &self.document_editor
    }

    /// Get a reference to the chat editor.
    pub fn chat_editor(&self) -> &Entity<Editor> {
        &self.chat_editor
    }

    /// Toggle the chat panel visibility.
    pub fn toggle_chat_panel(&mut self, cx: &mut Context<Self>) {
        self.chat_panel_visible = !self.chat_panel_visible;
        cx.notify();
    }

    /// Check if the chat panel is visible.
    pub fn is_chat_panel_visible(&self) -> bool {
        self.chat_panel_visible
    }

    /// Set chat panel visibility.
    pub fn set_chat_panel_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.chat_panel_visible != visible {
            self.chat_panel_visible = visible;
            cx.notify();
        }
    }
}

impl Focusable for AgentView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AgentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let chat_visible = self.chat_panel_visible;

        div()
            .id("agent-view")
            .key_context("AgentView")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &ToggleChatPanel, _window, cx| {
                this.toggle_chat_panel(cx);
            }))
            .on_action(cx.listener(|this, _: &SubmitPrompt, _window, cx| {
                this.submit_prompt(cx);
            }))
            .flex()
            .flex_row()
            .size_full()
            // Document editor - takes full width when chat hidden, or flex-1 when chat visible
            .child(
                div()
                    .id("document-pane")
                    .when(chat_visible, |d| d.flex_1().min_w_0())
                    .when(!chat_visible, |d| d.size_full())
                    .overflow_hidden()
                    .child(self.document_editor.clone()),
            )
            // Chat panel - only rendered when visible
            .when(chat_visible, |d| {
                d.child(
                    // Divider
                    div().w(px(1.0)).h_full().bg(rgb(0x44475a)),
                )
                .child(
                    div()
                        .id("chat-pane")
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        // Chat content area
                        .child(
                            div()
                                .id("chat-content")
                                .flex_1()
                                .min_h_0()
                                .overflow_hidden()
                                .child(self.chat_editor.clone()),
                        )
                        // Divider between chat and prompt
                        .child(div().h(px(1.0)).w_full().bg(rgb(0x44475a)))
                        // Prompt input area
                        .child(
                            div()
                                .id("prompt-input")
                                .h(px(100.0))
                                .min_h(px(100.0))
                                .overflow_hidden()
                                .child(self.prompt_editor.clone()),
                        ),
                )
            })
    }
}
