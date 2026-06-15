use std::path::PathBuf;
use std::sync::mpsc;

use log::error;
use notify::{RecursiveMode, Watcher};

use gpui::{AppContext, Context};

use crate::cursor::Selection;

use super::Editor;

impl Editor {
    /// Set up file watching for external changes.
    /// When the file changes externally, the buffer will be reloaded.
    /// If the file doesn't exist yet, watches the parent directory for its creation.
    pub fn watch_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.file_path = Some(path.clone());

        let (tx, rx) = mpsc::channel();
        let watch_path = path.clone();
        let file_exists = path.exists();

        let mut watcher = match notify::recommended_watcher(move |res: Result<notify::Event, _>| {
            if let Ok(event) = res {
                use notify::EventKind;
                match event.kind {
                    EventKind::Modify(_) => {
                        let _ = tx.send(());
                    }
                    EventKind::Create(_)
                        if event.paths.iter().any(|p| p == &watch_path) => {
                            let _ = tx.send(());
                        }
                    _ => {}
                }
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                error!("Failed to create file watcher: {}", e);
                return;
            }
        };

        let target = if file_exists {
            path.clone()
        } else if let Some(parent) = path.parent() {
            parent.to_path_buf()
        } else {
            error!("Cannot watch file with no parent directory: {:?}", path);
            return;
        };

        if let Err(e) = watcher.watch(&target, RecursiveMode::NonRecursive) {
            error!("Failed to watch {:?}: {}", target, e);
            return;
        }

        self.file_watcher_rx = Some(rx);
        self.file_watcher = Some(watcher);

        let windows = cx.windows();
        let watch_window = windows.first().cloned();
        cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(crate::config::FILE_WATCHER_POLL_MS))
                    .await;

                let mut continue_loop = true;
                    if !crate::file_ops::is_dialog_open()
                        && let Some(ref window) = watch_window {
                            continue_loop = cx
                                .update_window(*window, |_, _window, cx| {
                                    if let Some(editor) = weak.upgrade() {
                                        editor.update(cx, |editor, cx| {
                                            if let Some(rx) = &editor.file_watcher_rx {
                                                let mut changed = false;
                                                while rx.try_recv().is_ok() {
                                                    changed = true;
                                                }
                                                if changed {
                                                    editor.reload_file(cx);
                                                }
                                            }
                                        });
                                        true
                                    } else {
                                        false
                                    }
                                })
                                .unwrap_or(false);
                        }

                if !continue_loop {
                    break;
                }
            }
        })
        .detach();
    }

    /// Reload the file from disk, replacing buffer contents.
    fn reload_file(&mut self, cx: &mut Context<Self>) {
        let Some(path) = &self.file_path else { return };

        if let Some(last_save_mtime) = self.last_save_mtime
            && let Ok(metadata) = std::fs::metadata(path)
            && let Ok(file_mtime) = metadata.modified()
            && file_mtime == last_save_mtime
        {
            return;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to reload file {:?}: {}", path, e);
                return;
            }
        };

        if content != self.state.buffer.text() {
            self.set_text(&content, cx);
        }
    }

    /// Returns true if the buffer has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.state.buffer.is_dirty()
    }

    /// Mark the buffer as clean (no unsaved changes).
    pub fn mark_clean(&mut self) {
        self.state.buffer.mark_clean();
    }

    /// Save the buffer to the current file path, or prompt Save As if no path.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        if self.file_path.is_none() {
            self.save_as(cx);
            return;
        }

        let path = self.file_path.clone().unwrap();
        let content = self.state.buffer.text();

        if let Err(e) = std::fs::write(&path, &content) {
            error!("Failed to save file: {}", e);
            return;
        }

        self.state.buffer.mark_clean();
        self.last_save_mtime = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());
        cx.notify();
    }

    /// Save the buffer to a new path chosen via file dialog.
    pub fn save_as(&mut self, cx: &mut Context<Self>) {
        let default_name = self
            .file_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned());

        let Some(path) = crate::file_ops::pick_save_file(default_name.as_deref()) else {
            return;
        };

        let content = self.state.buffer.text();
        if let Err(e) = std::fs::write(&path, &content) {
            error!("Failed to save file: {}", e);
            return;
        }

        self.file_path = Some(path.clone());
        crate::user_config::add_recent_file(&path);
        self.state.buffer.mark_clean();
        self.last_save_mtime = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());

        if self.file_watcher.is_none() {
            self.watch_file(path.clone(), cx);
        }

        cx.notify();
    }

    /// Open a file at the given path, replacing current content.
    pub fn open_file_at(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to open file: {}", e);
                return;
            }
        };

        self.file_path = None;
        self.file_watcher = None;
        self.file_watcher_rx = None;

        self.set_text(&content, cx);
        self.state.buffer.mark_clean();
        self.file_path = Some(path.clone());
        self.watch_file(path.clone(), cx);

        crate::user_config::add_recent_file(&path);

        cx.notify();
    }

    /// Open a file chosen via file dialog, replacing current content.
    pub fn open_file(&mut self, cx: &mut Context<Self>) {
        if self.state.buffer.is_dirty() {
            match crate::file_ops::confirm_discard() {
                crate::file_ops::DiscardChoice::Save => self.save(cx),
                crate::file_ops::DiscardChoice::Cancel => return,
                crate::file_ops::DiscardChoice::DontSave => {}
            }
        }

        let Some(path) = crate::file_ops::pick_open_file() else {
            return;
        };
        self.open_file_at(path, cx);
    }

    /// Clear the editor to start a new file.
    pub fn new_file(&mut self, cx: &mut Context<Self>) {
        if self.state.buffer.is_dirty() {
            match crate::file_ops::confirm_discard() {
                crate::file_ops::DiscardChoice::Save => self.save(cx),
                crate::file_ops::DiscardChoice::Cancel => return,
                crate::file_ops::DiscardChoice::DontSave => {}
            }
        }

        self.file_path = None;
        self.file_watcher = None;
        self.file_watcher_rx = None;
        self.set_text("", cx);
        self.state.buffer.mark_clean();

        cx.notify();
    }

    /// Returns true if there are actions to undo.
    pub fn can_undo(&self) -> bool {
        self.state.buffer.can_undo()
    }

    /// Returns true if there are actions to redo.
    pub fn can_redo(&self) -> bool {
        self.state.buffer.can_redo()
    }

    /// Undo the last action.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if let Some(cursor_pos) = self.state.buffer.undo() {
            self.state.selection = Selection::new(cursor_pos, cursor_pos);
            cx.notify();
        }
    }

    /// Redo the last undone action.
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if let Some(cursor_pos) = self.state.buffer.redo() {
            self.state.selection = Selection::new(cursor_pos, cursor_pos);
            cx.notify();
        }
    }
}
