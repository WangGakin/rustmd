mod action;
mod config;
pub mod ime;
pub mod theme;

pub use action::{CenterLine, Direction, DispatchEditorAction, EditorAction};
pub use config::EditorConfig;
pub use theme::EditorTheme;

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use log::{error, warn};

/// Counter for generating unique editor instance IDs.
static NEXT_EDITOR_ID: AtomicUsize = AtomicUsize::new(0);

use gpui::{
    AnyElement, AnyWindowHandle, App, Context, Corner, CursorStyle, DragMoveEvent, Empty, FocusHandle, Focusable,
    Font, Hsla, IntoElement, KeyDownEvent, ListAlignment, ListState, ModifiersChangedEvent, MouseButton,
    Pixels, ReadGlobal, Render, Rgba, SharedString, TextRun, Window, anchored, div, font, list, point, prelude::*, px,
};

/// Marker type for text selection drag operations.
/// Used with GPUI's on_drag/on_drag_move to receive mouse events outside element bounds.
struct SelectionDrag;

/// Marker type for scrollbar drag operations.
struct ScrollbarDrag;

/// Empty view for the drag ghost (we don't need a visible drag indicator).
struct EmptyDragView;

impl Render for EmptyDragView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

use crate::line::CursorScreenPosition;
use crate::marker::MarkerKind;
use crate::status_bar::StatusBarInfo;

use crate::buffer::RenderSnapshot;
use crate::cursor::{Cursor, Selection};
use crate::inline::{
    NakedUrl, StyledRegion,
    detect_naked_urls,
};
use crate::line::{Line, LineParams, LineTheme};
use crate::paste::{PasteContext, transform_paste};
use crate::key_mode::KeyMode;

/// Context about the line at the cursor, used by smart editing actions.

mod state;
pub use state::*;


pub struct Editor {
    state: EditorState,
    focus_handle: FocusHandle,
    list_state: ListState,
    /// IME composition text range in byte offsets (None when not composing)
    pub(crate) ime_marked_range: Option<Range<usize>>,
    scroll_to_cursor_pending: bool,
    /// Last known cursor line, used to detect cursor movement for auto-scroll.
    last_cursor_line: Option<usize>,
    /// Last known cursor offset, used to detect cursor movement for autocomplete.
    last_cursor_offset: Option<usize>,
    input_blocked: bool,
    streaming_mode: bool,
    config: EditorConfig,
    /// Whether mouse is over a checkbox.
    hovering_checkbox: bool,
    /// Whether mouse is over a link (regardless of Ctrl state).
    hovering_link_region: bool,
    /// Whether Ctrl/Cmd is currently held.
    ctrl_held: bool,
    /// Last buffer version we synced to. Used to detect buffer changes.
    last_synced_version: u64,
    /// Last time we moved cursor during drag-scroll, for throttling.
    last_drag_scroll: Option<std::time::Instant>,
    /// True when we're in the drag-scroll zone, to prevent line's on_drag from resetting selection.
    in_drag_scroll_zone: bool,
    /// True while actively dragging a selection. Used to prevent marker oscillation.
    /// Once set, stays true until mouse up to keep markers expanded.
    is_selecting: bool,
    /// Y coordinate where scrollbar thumb drag started (None when not dragging).
    scrollbar_drag_start_y: Option<Pixels>,
    /// True when scrollbar click hasn't yet been classified as drag or page-turn.
    scrollbar_pending_page_turn: bool,
    /// Path to the file being edited (if any).
    file_path: Option<PathBuf>,
    /// Receiver for file watcher events.
    file_watcher_rx: Option<mpsc::Receiver<()>>,
    /// File watcher handle (kept alive to maintain the watch).
    #[allow(dead_code)]
    file_watcher: Option<notify::RecommendedWatcher>,
    /// The mtime of the file after our last save (used to detect external vs our own changes).
    last_save_mtime: Option<std::time::SystemTime>,

    /// Autocomplete popup state.
    autocomplete: Option<AutocompleteState>,
    /// Pending autocomplete fetch (for debouncing).
    autocomplete_debounce_task: Option<gpui::Task<()>>,
    /// Whether this is the primary editor that updates global state (status bar, title bar).
    /// Only one editor should have this set to true at a time.
    is_primary: bool,
    /// Per-editor status bar info (replaces global StatusBarInfo).
    status_info: StatusBarInfo,
    /// Window handle for async operations (replaces cx.windows().first()).
    window_handle: Option<AnyWindowHandle>,
    /// Shared cursor screen position (written by Line paint, read by autocomplete popup).
    cursor_screen_pos: Rc<RefCell<CursorScreenPosition>>,
    /// Unique instance ID for element IDs to prevent GPUI element caching conflicts.
    instance_id: usize,
    /// Line ranges that are user messages (for chat editor background highlighting).
    /// Each range is start_line..end_line (exclusive).
    user_message_lines: Vec<Range<usize>>,
    /// Controls cursor blink visibility. Toggled by a background timer.
    cursor_blink_visible: bool,
}

impl Editor {
    /// Create a new editor with the given content and default configuration.
    pub fn new(content: &str, cx: &mut Context<Self>) -> Self {
        Self::with_config(content, EditorConfig::default(), cx)
    }

    /// Create a new editor with the given content and configuration.
    pub fn with_config(content: &str, config: EditorConfig, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let state = EditorState::new(content);
        let line_count = state.buffer.line_count();
        let list_state = ListState::new(line_count, ListAlignment::Top, px(200.0));

        

        Self {
            state,
            focus_handle,
            list_state,
            scroll_to_cursor_pending: false,
            last_cursor_line: None,
            last_cursor_offset: None,
            input_blocked: false,
            streaming_mode: false,
            config,
            hovering_checkbox: false,
            hovering_link_region: false,
            ctrl_held: false,
            ime_marked_range: None,
            last_synced_version: 0,
            last_drag_scroll: None,
            in_drag_scroll_zone: false,
            is_selecting: false,
            scrollbar_drag_start_y: None,
            scrollbar_pending_page_turn: false,
            file_path: None,
            file_watcher_rx: None,
            file_watcher: None,
            last_save_mtime: None,
            autocomplete: None,
            autocomplete_debounce_task: None,
            is_primary: true, // Default to primary; secondary editors should call set_primary(false)
            status_info: StatusBarInfo::default(),
            window_handle: None,
            cursor_screen_pos: Rc::new(RefCell::new(CursorScreenPosition::default())),
            instance_id: NEXT_EDITOR_ID.fetch_add(1, Ordering::Relaxed),
            user_message_lines: Vec::new(),
            cursor_blink_visible: true,
        }
    }

    /// Start a background timer that toggles cursor visibility every 500ms.
    pub fn start_cursor_blink(&mut self, handle: AnyWindowHandle, cx: &mut Context<Self>) {
        self.window_handle = Some(handle);
        cx.spawn(async move |this, cx| {
            loop {
                if !crate::file_ops::is_dialog_open() {
                    let _ = cx.update_window(handle, |_, _window, cx| {
                        if let Some(editor) = this.upgrade() {
                            editor.update(cx, |editor, cx| {
                                editor.cursor_blink_visible = !editor.cursor_blink_visible;
                                cx.notify();
                            });
                        }
                    });
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(crate::config::CURSOR_BLINK_MS))
                    .await;
            }
        })
        .detach();
    }

    /// Reset blink state â€” cursor becomes visible and blink cycle restarts.
    fn reset_cursor_blink(&mut self) {
        self.cursor_blink_visible = true;
    }

    /// Set whether this editor is the primary editor that updates global state.
    /// Only the primary editor should update StatusBarInfo and FileInfo globals.
    pub fn set_primary(&mut self, is_primary: bool) {
        self.is_primary = is_primary;
    }

    /// Get a render snapshot of the current buffer state.
    /// Useful for capturing state before agent edits.
    pub fn render_snapshot(&mut self) -> RenderSnapshot {
        self.state.buffer.render_snapshot()
    }

    /// Get the file path this editor is editing, if any.
    pub fn file_path(&self) -> Option<&PathBuf> {
        self.file_path.as_ref()
    }

    /// Get a reference to the per-editor status bar info.
    pub fn status_info(&self) -> &StatusBarInfo {
        &self.status_info
    }



    /// Detect naked URLs in a range of lines.
    /// Returns URLs indexed by line number.
    fn detect_naked_urls_in_range(
        &mut self,
        start_line: usize,
        end_line: usize,
    ) -> HashMap<usize, Vec<NakedUrl>> {
        let snapshot = self.state.buffer.render_snapshot();
        let mut urls_by_line = HashMap::new();

        for line_idx in start_line..end_line.min(snapshot.line_count()) {
            let line = snapshot.line_markers(line_idx);
            let line_range = line.range.clone();
            let line_text = snapshot
                .rope
                .slice(
                    snapshot.rope.byte_to_char(line_range.start)
                        ..snapshot.rope.byte_to_char(line_range.end),
                )
                .to_string();

            let inline_styles = snapshot.inline_styles_for_line(line_idx);

            let code_ranges: Vec<_> = inline_styles
                .iter()
                .filter(|s| s.style.code)
                .map(|s| s.full_range.clone())
                .collect();

            let link_ranges: Vec<_> = inline_styles
                .iter()
                .filter(|s| s.link_url.is_some())
                .map(|s| s.full_range.clone())
                .collect();

            let urls = detect_naked_urls(&line_text, line_range.start, &code_ranges, &link_ranges);
            if !urls.is_empty() {
                urls_by_line.insert(line_idx, urls);
            }
        }

        urls_by_line
    }



    /// Fetch autocomplete suggestions after a debounce delay.
    /// Cancels any pending fetch and starts a new timer.
    fn fetch_autocomplete_suggestions_debounced(&mut self, cx: &mut Context<Self>) {
        self.autocomplete_debounce_task = None;
        let window = self.window_handle;
        let task = cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(crate::config::AUTOCOMPLETE_DEBOUNCE_MS))
                .await;
            if crate::file_ops::is_dialog_open() {
                return;
            }
            if let Some(window) = window {
                let _ = cx.update_window(window, |_, _window, cx| {
                    if let Some(editor) = weak.upgrade() {
                        editor.update(cx, |_editor, cx| {
                            cx.notify();
                        });
                    }
                });
            }
        });
        self.autocomplete_debounce_task = Some(task);
    }



    /// Set up file watching for external changes.
    /// When the file changes externally, the buffer will be reloaded.
    /// If the file doesn't exist yet, watches the parent directory for its creation.
    pub fn watch_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        use notify::{RecursiveMode, Watcher};

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

    /// Returns the buffer contents as a string.
    pub fn text(&self) -> String {
        self.state.buffer.text()
    }

    /// Returns the length of the buffer in bytes.
    pub fn len(&self) -> usize {
        self.state.buffer.len_bytes()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.state.buffer.len_bytes() == 0
    }

    /// Check if buffer ends with the given string (efficient, doesn't copy whole buffer).
    pub fn ends_with(&self, suffix: &str) -> bool {
        self.state.buffer.ends_with(suffix)
    }

    /// Find the nearest heading above or at the given line.
    /// Returns the heading level (1-6) if found, None otherwise.
    fn find_current_heading(&self, from_line: usize) -> Option<u8> {
        for line_idx in (0..=from_line).rev() {
            let markers = self.state.buffer.line_markers(line_idx);
            for marker in &markers.markers {
                if let MarkerKind::Heading(level) = marker.kind {
                    return Some(level);
                }
            }
        }
        None
    }

    /// Replace the entire buffer contents, resetting cursor to the start.
    pub fn set_text(&mut self, content: &str, cx: &mut Context<Self>) {
        self.state.buffer = content.parse().unwrap_or_default();
        self.state.selection = Selection::new(0, 0);
        cx.notify();
    }

    /// Sync the list state count with the buffer line count.
    /// Also triggers autosave if enabled.
    fn sync_list_state(&mut self, cx: &mut Context<Self>) {
        let line_count = self.state.buffer.line_count();
        let current_count = self.list_state.item_count();

        if line_count != current_count {
            if line_count > current_count {
                self.list_state
                    .splice(current_count..current_count, line_count - current_count);
            } else {
                self.list_state.splice(line_count..current_count, 0);
            }
        }

        let config = crate::config::Config::global(cx);
        if config.autosave {
            self.save(cx);
            if let Some(path) = &self.file_path
                && let Ok(metadata) = std::fs::metadata(path)
            {
                self.last_save_mtime = metadata.modified().ok();
            }
        }
    }

    /// Insert text at the current cursor position.
    pub fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        self.insert_text(text);
        cx.notify();
    }

    /// Append text to the end of the buffer and move cursor to the end.
    ///
    /// Useful for streaming content from an AI or other source.
    pub fn append(&mut self, text: &str, cx: &mut Context<Self>) {
        let end = self.state.buffer.len_bytes();
        self.state.buffer.insert(end, text, end);
        let new_end = self.state.buffer.len_bytes();
        self.state.selection = Selection::new(new_end, new_end);
        cx.notify();
    }

    /// Append a user message to the end of the buffer, tracking its line range
    /// for background highlighting. Trailing empty lines are not included in the range.
    pub fn append_user_message(&mut self, text: &str, cx: &mut Context<Self>) {
        let line_count_before = self.state.buffer.line_count();
        self.append(text, cx);

        // The content we just added spans from the old last line to (new_count - trailing_blanks)
        let content_lines = text.trim_end().lines().count();
        // Content starts at line_count_before - 1 (the old empty last line)
        let start_line = line_count_before.saturating_sub(1);
        let end_line = start_line + content_lines;

        if end_line > start_line {
            self.user_message_lines.push(start_line..end_line);
        }
    }

    /// Check if a line is part of a user message.
    pub fn is_user_message_line(&self, line: usize) -> bool {
        self.user_message_lines.iter().any(|r| r.contains(&line))
    }

    /// Append text and scroll to keep the cursor visible.
    pub fn append_and_scroll(&mut self, text: &str, _window: &mut Window, cx: &mut Context<Self>) {
        self.append(text, cx);
        self.request_scroll_to_cursor();
    }

    fn cursor(&self) -> Cursor {
        self.state.selection.cursor()
    }

    fn move_cursor(&mut self, new_cursor: Cursor, extend: bool) {
        if extend {
            self.state.selection = self.state.selection.extend_to(new_cursor.offset);
        } else {
            self.state.selection = Selection::new(new_cursor.offset, new_cursor.offset);
        }
    }

    fn request_scroll_to_cursor(&mut self) {
        self.scroll_to_cursor_pending = true;
    }

    fn tab(&mut self) {
        self.state.tab();
    }

    fn shift_tab(&mut self) {
        self.state.shift_tab();
    }

    fn toggle_checkbox(&mut self, line_number: usize, cx: &mut Context<Self>) {
        self.state.toggle_checkbox_state(line_number);
        cx.notify();
    }

    fn insert_text(&mut self, text: &str) {
        self.state.insert_text(text);
    }

    /// Try to detect an autocomplete trigger at the given position in line_text.
    /// Returns Some((trigger_type, trigger_offset, prefix)) if found.
    fn detect_autocomplete_trigger(
        line_text: &str,
        line_start: usize,
    ) -> Option<(AutocompleteTrigger, usize, String)> {
        // Try each trigger character, preferring the rightmost one
        let triggers = [
            ('@', AutocompleteTrigger::User),
        ];

        let mut best_match: Option<(AutocompleteTrigger, usize, String)> = None;

        for (trigger_char, trigger_type) in triggers {
            if let Some(pos) = line_text.rfind(trigger_char) {
                // Check word boundary
                let is_at_word_boundary = pos == 0
                    || line_text
                        .as_bytes()
                        .get(pos - 1)
                        .is_none_or(|&b| b == b' ' || b == b'\t' || b == b'\n');

                if !is_at_word_boundary {
                    continue;
                }

                let prefix = line_text[pos + 1..].to_string();

                // Validate prefix based on trigger type
                let valid = match trigger_type {
                    AutocompleteTrigger::User => {
                        // @ prefix: alphanumeric and hyphens, not starting with hyphen
                        prefix.is_empty()
                            || (prefix.chars().all(|c| c.is_alphanumeric() || c == '-')
                                && !prefix.starts_with('-'))
                    }
                };

                if !valid {
                    continue;
                }

                let trigger_offset = line_start + pos;

                // Keep the rightmost valid trigger
                if best_match
                    .as_ref()
                    .map(|(_, off, _)| trigger_offset > *off)
                    .unwrap_or(true)
                {
                    best_match = Some((trigger_type, trigger_offset, prefix));
                }
            }
        }

        best_match
    }

    /// Check if cursor position triggers autocomplete.
    /// Returns true if we should fetch suggestions.
    fn update_autocomplete_from_cursor(&mut self) -> bool {
        let cursor = self.state.cursor().offset;

        // Detect trigger from raw text
        if cursor > 0 {
            let cursor_line = self.state.buffer.byte_to_line(cursor);
            let line_start = self.state.buffer.line_to_byte(cursor_line);
            let line_text = self.state.buffer.slice_cow(line_start..cursor).into_owned();

            if let Some((trigger, trigger_offset, prefix)) =
                Self::detect_autocomplete_trigger(&line_text, line_start)
            {
                return self.set_autocomplete_state(trigger, trigger_offset, prefix);
            }
        }

        // Cursor not inside any ref - close autocomplete
        self.autocomplete = None;
        false
    }

    /// Update autocomplete state for a detected trigger.
    /// Returns true if suggestions should be fetched/filtered.
    fn set_autocomplete_state(
        &mut self,
        trigger: AutocompleteTrigger,
        trigger_offset: usize,
        prefix: String,
    ) -> bool {
        // Check if state actually changed
        let changed = self
            .autocomplete
            .as_ref()
            .map(|ac| ac.trigger != trigger || ac.prefix != prefix)
            .unwrap_or(true);

        if !changed {
            return false;
        }

        // Preserve old state only if same trigger type
        let old_state = self.autocomplete.take();
        let same_trigger = old_state
            .as_ref()
            .map(|ac| ac.trigger == trigger)
            .unwrap_or(false);

        self.autocomplete = Some(AutocompleteState {
            trigger,
            trigger_offset,
            prefix,
            suggestions: old_state
                .as_ref()
                .filter(|_| same_trigger)
                .map(|ac| ac.suggestions.clone())
                .unwrap_or_default(),
            selected_index: old_state
                .as_ref()
                .filter(|_| same_trigger)
                .map(|ac| ac.selected_index)
                .unwrap_or(0),
            loading: false,
            fetched_prefix: old_state
                .filter(|_| same_trigger)
                .and_then(|ac| ac.fetched_prefix),
        });

        true
    }

    /// Render the autocomplete popup if active.
    fn render_autocomplete(
        &self,
        line_theme: &LineTheme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let ac = self.autocomplete.as_ref()?;

        // Don't show popup if loading or no suggestions
        if ac.loading || ac.suggestions.is_empty() {
            return None;
        }

        let theme = &self.config.theme;

        // Get absolute cursor position (set during Line paint)
        let cursor_screen_pos = self.cursor_screen_pos.borrow();
        let cursor_pos = cursor_screen_pos.position?;
        drop(cursor_screen_pos);

        // Get viewport bounds for fallback
        let viewport = self.list_state.viewport_bounds();

        let popup_width: Option<Pixels> = None;
        let popup_max_height = px(300.0);
        let gap = px(4.0);

        // Position Y below the cursor row, or flip above if not enough space below
        let line_height = self.config.line_height.to_pixels(window.rem_size());
        let viewport_bottom = viewport.origin.y + viewport.size.height;
        let space_below = viewport_bottom - (cursor_pos.y + line_height);
        let space_above = cursor_pos.y - viewport.origin.y;

        let (popup_y, anchor_corner) = if space_below >= popup_max_height + gap {
            // Enough space below - position popup below cursor
            (cursor_pos.y + line_height + gap, Corner::TopLeft)
        } else if space_above >= popup_max_height + gap {
            // Not enough below but enough above - flip to above cursor
            (cursor_pos.y - gap, Corner::BottomLeft)
        } else {
            // Not enough space either way - default to below
            (cursor_pos.y + line_height + gap, Corner::TopLeft)
        };

        // Build suggestion items
        let border_color = theme.comment;
        let suggestion_count = ac.suggestions.len();
        let selection_bg = theme.selection;

        let items: Vec<AnyElement> = ac
            .suggestions
            .iter()
            .enumerate()
            .map(|(i, suggestion)| {
                let is_selected = i == ac.selected_index;
                let is_first = i == 0;
                let is_last = i == suggestion_count - 1;

                // Build display element based on suggestion type
                let display_element: AnyElement = match suggestion {
                    AutocompleteSuggestion::User { login, name } => {
                        // Styled text for users: cyan "@login" + dimmed "Display Name"
                        let mut row = div().flex().flex_row().gap_1();
                        row = row.child(div().text_color(theme.cyan).child(format!("@{}", login)));
                        if let Some(n) = name {
                            row = row.child(div().text_color(theme.comment).child(n.clone()));
                        }
                        row.into_any_element()
                    }
                };

                div()
                    .id(("autocomplete-item", i))
                    .px_2()
                    .py_1()
                    .when_some(popup_width, |d, w| d.w(w))
                    .cursor_pointer()
                    .when(is_first, |d| d.rounded_t_md())
                    .when(is_last, |d| d.rounded_b_md())
                    .when(!is_last, |d| d.border_b_1().border_color(border_color))
                    .when(is_selected, |d| d.bg(selection_bg))
                    .hover(|d| d.bg(selection_bg))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |editor, _event, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            if let Some(ref mut ac) = editor.autocomplete {
                                ac.selected_index = i;
                            }
                            if editor.accept_autocomplete_suggestion() {
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(move |editor, _event, _window, cx| {
                        if let Some(ref mut ac) = editor.autocomplete
                            && ac.selected_index != i
                        {
                            ac.selected_index = i;
                            cx.notify();
                        }
                    }))
                    .child(display_element)
                    .into_any_element()
            })
            .collect();

        Some(
            anchored()
                .position(point(cursor_pos.x, popup_y))
                .anchor(anchor_corner)
                .child(
                    div()
                        .id("autocomplete-popup")
                        .bg(theme.background)
                        .border_1()
                        .border_color(theme.comment)
                        .rounded_md()
                        .shadow_md()
                        .overflow_hidden()
                        .when_some(popup_width, |d, w| d.w(w))
                        .max_h(px(300.0))
                        .overflow_y_scroll()
                        .text_size(px(14.0))
                        .font(line_theme.text_font.clone())
                        .on_scroll_wheel(cx.listener(|_editor, _event, _window, cx| {
                            cx.stop_propagation();
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|_editor, _event, _window, cx| {
                                cx.stop_propagation();
                            }),
                        )
                        .children(items),
                )
                .into_any_element(),
        )
    }



    /// Accept the currently selected autocomplete suggestion.
    /// Returns true if a suggestion was accepted.
    fn accept_autocomplete_suggestion(&mut self) -> bool {
        let ac = match self.autocomplete.take() {
            Some(ac) => ac,
            None => return false,
        };

        if ac.suggestions.is_empty() {
            return false;
        }

        let suggestion = &ac.suggestions[ac.selected_index];
        let replacement = match suggestion {
            AutocompleteSuggestion::User { login, .. } => format!("@{}", login),
        };

        // Replace text from trigger_offset to current cursor with the replacement
        let cursor = self.state.cursor().offset;
        let range = ac.trigger_offset..cursor;
        self.state.buffer.delete(range.clone(), cursor);
        self.state
            .buffer
            .insert(ac.trigger_offset, &replacement, ac.trigger_offset);
        let new_pos = ac.trigger_offset + replacement.len();
        self.state.selection = Selection::new(new_pos, new_pos);

        true
    }

    fn delete_backward(&mut self) {
        self.state.delete_backward();
    }

    fn delete_forward(&mut self) {
        self.state.delete_forward();
    }

    fn delete_to_line_end(&mut self) {
        let cursor_pos = self.cursor().offset;
        let line_end = self.cursor().move_to_line_end(&self.state.buffer).offset;
        if cursor_pos < line_end {
            self.state.buffer.delete(cursor_pos..line_end, cursor_pos);
        }
    }

    fn enter(&mut self) {
        self.state.enter();
        self.scroll_to_cursor_pending = true;
    }

    fn shift_enter(&mut self) {
        self.state.shift_enter();
        self.scroll_to_cursor_pending = true;
    }

    fn shift_alt_enter(&mut self) {
        self.state.shift_alt_enter();
        self.scroll_to_cursor_pending = true;
    }

    fn move_in_direction(&mut self, direction: Direction, extend: bool) {
        let new_cursor = match direction {
            Direction::Left => self.cursor().move_left(&self.state.buffer),
            Direction::Right => self.cursor().move_right(&self.state.buffer),
            Direction::Up => self.cursor().move_up(&self.state.buffer),
            Direction::Down => self.cursor().move_down(&self.state.buffer),
        };

        self.move_cursor(new_cursor, extend);
        self.scroll_to_cursor_pending = true;
    }

    fn move_in_direction_visual(
        &mut self,
        direction: Direction,
        extend: bool,
        window: &mut Window,
    ) {
        let (Direction::Up | Direction::Down) = direction else {
            self.move_in_direction(direction, extend);
            return;
        };

        let cursor_offset = self.state.cursor().offset;
        let current_line_idx = self.state.buffer.byte_to_line(cursor_offset);
        let line_range = self.state.buffer.line_byte_range(current_line_idx);
        let line_text = self.state.buffer.slice_cow(line_range.clone()).into_owned();
        let cursor_in_line = cursor_offset - line_range.start;

        let rem_size = window.rem_size();
        let viewport_width = self.list_state.viewport_bounds().size.width;
        let max_width = self.config.max_line_width.unwrap_or(viewport_width);
        let padding_x = self.config.padding_x.to_pixels(rem_size);
        let available_width = (max_width.min(viewport_width) - padding_x * 2.0).max(px(1.0));

        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(rem_size);
        let line_font = font(&self.config.text_font);

        let wrap_offsets = Self::compute_wrap_offsets(
            &line_text,
            available_width,
            &line_font,
            font_size,
            window,
        );

        let visual_row = wrap_offsets
            .iter()
            .position(|&o| o > cursor_in_line)
            .unwrap_or(wrap_offsets.len());

        let run = TextRun {
            len: line_text.len(),
            font: line_font.clone(),
            color: gpui::transparent_black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shared: SharedString = line_text.into();
        let shaped = window
            .text_system()
            .shape_line(shared.clone(), font_size, &[run], None);

        let current_row_start_byte = if visual_row == 0 {
            0
        } else {
            wrap_offsets[visual_row - 1]
        };
        let row_start_x = shaped.x_for_index(current_row_start_byte);
        let relative_x = if cursor_in_line >= current_row_start_byte {
            shaped.x_for_index(cursor_in_line) - row_start_x
        } else {
            px(0.0)
        };

        let line_text = shared; // reuse for length
        let line_len = line_text.len();

        // Helper: on a given shaped line, find the byte offset at visual x
        // within [row_start..row_end).
        let offset_at_x = |shaped: &gpui::ShapedLine,
                           row_start: usize,
                           row_end: usize,
                           x: Pixels|
         -> usize {
            let target_x = shaped.x_for_index(row_start) + x;
            shaped
                .index_for_x(target_x)
                .unwrap_or(row_start)
                .min(row_end)
                .max(row_start)
        };

        let new_cursor_opt = if direction == Direction::Down {
            if visual_row < wrap_offsets.len() {
                let row_start = wrap_offsets[visual_row];
                let row_end = if visual_row + 1 < wrap_offsets.len() {
                    wrap_offsets[visual_row + 1]
                } else {
                    line_len
                };
                let idx = offset_at_x(&shaped, row_start, row_end, relative_x);
                Some(Cursor {
                    offset: line_range.start + idx,
                })
            } else {
                // Last visual row â€” cross into next buffer line
                let target_line = current_line_idx + 1;
                if target_line >= self.state.buffer.line_count() {
                    None
                } else {
                    self.visual_cross_line(
                        target_line,
                        relative_x,
                        available_width,
                        &line_font,
                        font_size,
                        /* from_end = */ false,
                        window,
                    )
                }
            }
        } else if visual_row > 0 {
            let prev_row_start = if visual_row == 1 {
                0
            } else {
                wrap_offsets[visual_row - 2]
            };
            let row_end = wrap_offsets[visual_row - 1];
            let idx = offset_at_x(&shaped, prev_row_start, row_end, relative_x);
            Some(Cursor {
                offset: line_range.start + idx,
            })
        } else {
            // First visual row â€” cross into previous buffer line
            if current_line_idx == 0 {
                None
            } else {
                let target_line = current_line_idx - 1;
                self.visual_cross_line(
                    target_line,
                    relative_x,
                    available_width,
                    &line_font,
                    font_size,
                    /* from_end = */ true,
                    window,
                )
            }
        };

        match new_cursor_opt {
            Some(new_cursor) => {
                self.move_cursor(new_cursor, extend);
                self.scroll_to_cursor_pending = true;
            }
            None => {
                self.move_in_direction(direction, extend);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn visual_cross_line(
        &mut self,
        target_line_idx: usize,
        visual_x: Pixels,
        available_width: Pixels,
        font: &Font,
        font_size: Pixels,
        from_end: bool,
        window: &mut Window,
    ) -> Option<Cursor> {
        let target_range = self.state.buffer.line_byte_range(target_line_idx);
        let target_text = self
            .state
            .buffer
            .slice_cow(target_range.clone())
            .into_owned();
        if target_text.is_empty() {
            return Some(Cursor {
                offset: target_range.start,
            });
        }

        let wrap_offsets = Self::compute_wrap_offsets(
            &target_text,
            available_width,
            font,
            font_size,
            window,
        );

        let run = TextRun {
            len: target_text.len(),
            font: font.clone(),
            color: gpui::transparent_black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shared: SharedString = target_text.into();
        let shaped = window
            .text_system()
            .shape_line(shared.clone(), font_size, &[run], None);

        let line_len = shared.len();
        if from_end && !wrap_offsets.is_empty() {
            // Enter the LAST visual row
            let last_row_start = wrap_offsets.last().copied().unwrap_or(0);
            let target_x = shaped.x_for_index(last_row_start) + visual_x;
            let idx = shaped
                .index_for_x(target_x)
                .unwrap_or(line_len)
                .min(line_len)
                .max(last_row_start);
            Some(Cursor {
                offset: target_range.start + idx,
            })
        } else {
            // Enter the FIRST visual row (or the only row if no wrapping)
            let row_end = wrap_offsets.first().copied().unwrap_or(line_len);
            let target_x = shaped.x_for_index(0) + visual_x;
            let idx = shaped
                .index_for_x(target_x)
                .unwrap_or(0)
                .min(row_end);
            Some(Cursor {
                offset: target_range.start + idx,
            })
        }
    }

    fn compute_wrap_offsets(
        text: &str,
        available_width: Pixels,
        font: &Font,
        font_size: Pixels,
        window: &mut Window,
    ) -> Vec<usize> {
        if text.is_empty() || available_width <= px(0.0) {
            return Vec::new();
        }

        let shared: SharedString = text.to_string().into();
        let run = TextRun {
            len: shared.len(),
            font: font.clone(),
            color: gpui::transparent_black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_line(shared.clone(), font_size, &[run], None);

        if shaped.width <= available_width {
            return Vec::new();
        }

        let text_len = shared.len();
        let mut offsets = Vec::new();
        let mut current_row_start = 0;

        while current_row_start < text_len {
            let start_x = shaped.x_for_index(current_row_start);
            let end_x = start_x + available_width;

            if end_x >= shaped.width {
                break;
            }

            let Some(idx) = shaped.index_for_x(end_x) else { break };
            let wrap_idx = if idx <= current_row_start {
                // Can't fit even one more char; advance minimally past char boundary
                let mut w = current_row_start + 1;
                while w < text_len && !shared.is_char_boundary(w) {
                    w += 1;
                }
                w
            } else {
                let mut w = idx;
                while w < text_len && !shared.is_char_boundary(w) {
                    w += 1;
                }
                w
            };

            if wrap_idx >= text_len || wrap_idx <= current_row_start {
                break;
            }

            current_row_start = wrap_idx;
            offsets.push(current_row_start);
        }

        offsets
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.input_blocked {
            return;
        }
        self.reset_cursor_blink();

        // Defensive: if IME marked range went stale (e.g. IME cancelled without cleanup),
        // discard it so GPUI resumes normal keyboard dispatch.
        if let Some(ref mark) = self.ime_marked_range {
            let buf_len = self.state.buffer.len_bytes();
            if mark.start > buf_len || mark.end > buf_len {
                self.ime_marked_range = None;
            }
        }

        let keystroke = &event.keystroke;

        // Handle autocomplete keyboard navigation
        if self.autocomplete.is_some() {
            match keystroke.key.as_str() {
                "escape" => {
                    self.autocomplete = None;
                    cx.notify();
                    return;
                }
                "up" => {
                    if let Some(ref mut ac) = self.autocomplete
                        && !ac.suggestions.is_empty()
                    {
                        if ac.selected_index == 0 {
                            ac.selected_index = ac.suggestions.len() - 1;
                        } else {
                            ac.selected_index -= 1;
                        }
                        cx.notify();
                        return;
                    }
                }
                "down" => {
                    if let Some(ref mut ac) = self.autocomplete
                        && !ac.suggestions.is_empty()
                    {
                        ac.selected_index = (ac.selected_index + 1) % ac.suggestions.len();
                        cx.notify();
                        return;
                    }
                }
                "enter" | "tab" => {
                    // Only accept if popup is visible (has suggestions)
                    if let Some(ref ac) = self.autocomplete
                        && !ac.suggestions.is_empty()
                        && self.accept_autocomplete_suggestion()
                    {
                        cx.notify();
                        return;
                    }
                }
                _ => {}
            }
        }

        let extend = keystroke.modifiers.shift;
        let is_mac_mode = KeyMode::is_mac(cx);
        let is_ctrl = keystroke.modifiers.control;
        let is_ctrl_shift = keystroke.modifiers.control && keystroke.modifiers.shift;

        // Mac mode: Ctrl+letter shortcuts
        if is_mac_mode && is_ctrl && !keystroke.modifiers.alt {
            match keystroke.key.as_str() {
                "a" if !keystroke.modifiers.shift => {
                    let new_cursor = self.cursor().move_to_line_start(&self.state.buffer);
                    self.move_cursor(new_cursor, extend);
                    self.scroll_to_cursor_pending = true;
                    cx.notify();
                    return;
                }
                "e" => {
                    let new_cursor = self.cursor().move_to_line_end(&self.state.buffer);
                    self.move_cursor(new_cursor, extend);
                    self.scroll_to_cursor_pending = true;
                    cx.notify();
                    return;
                }
                "b" => {
                    self.move_in_direction(Direction::Left, extend);
                    cx.notify();
                    return;
                }
                "f" => {
                    self.move_in_direction(Direction::Right, extend);
                    cx.notify();
                    return;
                }
                "p" => {
                    self.move_in_direction_visual(Direction::Up, extend, window);
                    cx.notify();
                    return;
                }
                "n" => {
                    self.move_in_direction_visual(Direction::Down, extend, window);
                    cx.notify();
                    return;
                }
                "d" => {
                    self.delete_forward();
                    cx.notify();
                    return;
                }
                "h" => {
                    self.delete_backward();
                    cx.notify();
                    return;
                }
                "k" => {
                    self.delete_to_line_end();
                    cx.notify();
                    return;
                }
                _ => {}
            }
        }

        match keystroke.key.as_str() {
            "backspace" => {
                self.delete_backward();
            }
            "delete" => {
                self.delete_forward();
            }
            "left" => {
                self.move_in_direction(Direction::Left, extend);
            }
            "right" => {
                self.move_in_direction(Direction::Right, extend);
            }
            "up" => {
                self.move_in_direction_visual(Direction::Up, extend, window);
            }
            "down" => {
                self.move_in_direction_visual(Direction::Down, extend, window);
            }
            "home" => {
                let new_cursor = if keystroke.modifiers.control || keystroke.modifiers.platform {
                    self.cursor().move_to_start()
                } else {
                    self.cursor().move_to_line_start(&self.state.buffer)
                };
                self.move_cursor(new_cursor, extend);
                self.scroll_to_cursor_pending = true;
            }
            "end" => {
                let new_cursor = if keystroke.modifiers.control || keystroke.modifiers.platform {
                    self.cursor().move_to_end(&self.state.buffer)
                } else {
                    self.cursor().move_to_line_end(&self.state.buffer)
                };
                self.move_cursor(new_cursor, extend);
                self.scroll_to_cursor_pending = true;
            }
            "enter" => {
                if keystroke.modifiers.shift && keystroke.modifiers.alt {
                    self.shift_alt_enter();
                } else if keystroke.modifiers.shift {
                    self.shift_enter();
                } else {
                    self.enter();
                }
            }
            "space" => {
                if !self.state.try_insert_space() {
                    return;
                }
            }
            "tab" => {
                if self.state.cursor_in_code_block() {
                    self.insert_text("    ");
                } else if keystroke.modifiers.shift {
                    self.shift_tab();
                } else {
                    self.tab();
                }
            }
            "a" if (keystroke.modifiers.control || keystroke.modifiers.platform)
                && (!is_mac_mode || is_ctrl_shift) =>
            {
                self.state.selection = Selection::select_all(&self.state.buffer);
            }
            "c" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if !self.state.selection.is_collapsed() {
                    let range = self.state.selection.range();
                    let text = self.state.buffer.slice_cow(range).into_owned();
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                }
            }
            "x" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if !self.state.selection.is_collapsed() {
                    let range = self.state.selection.range();
                    let text = self.state.buffer.slice_cow(range).into_owned();
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    self.state.delete_selection();
                }
            }
            "v" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if let Some(clipboard_item) = cx.read_from_clipboard()
                    && let Some(text) = clipboard_item.text()
                {
                    let ctx =
                        PasteContext::from_buffer(&self.state.buffer, self.state.cursor().offset);
                    let transformed = transform_paste(&text, &ctx);
                    self.insert_text(&transformed);
                }
            }
            "z" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if keystroke.modifiers.shift {
                    if let Some(cursor_pos) = self.state.buffer.redo() {
                        self.state.selection = Selection::new(cursor_pos, cursor_pos);
                    }
                } else if let Some(cursor_pos) = self.state.buffer.undo() {
                    self.state.selection = Selection::new(cursor_pos, cursor_pos);
                }
            }
            "y" if keystroke.modifiers.control => {
                if let Some(cursor_pos) = self.state.buffer.redo() {
                    self.state.selection = Selection::new(cursor_pos, cursor_pos);
                }
            }
            _ => {
                if let Some(key_char) = &keystroke.key_char {
                    if key_char == " "
                        && !self.state.try_insert_space() {
                            return;
                        }
                    // Regular text insertion is handled by WM_CHAR ->
                    // replace_text_in_range. on_key_down does not insert
                    // printable characters to avoid the WM_KEYDOWN/WM_CHAR
                    // double-path conflict that causes IME bugs.

                    if key_char == ">" {
                        self.state.maybe_complete_blockquote_marker();
                    }

                    if key_char == "`" || key_char == "~" {
                        self.state.maybe_complete_code_fence();
                    }

                    self.scroll_to_cursor_pending = true;
                }
            }
        }

        cx.notify();
    }

    fn on_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ctrl_held = event.modifiers.control || event.modifiers.platform;
        if self.ctrl_held != ctrl_held {
            self.ctrl_held = ctrl_held;
            cx.notify();
        }
    }

    /// Block or unblock user input. Useful during demos or streaming.
    pub fn set_input_blocked(&mut self, blocked: bool) {
        self.input_blocked = blocked;
    }

    /// Returns true if user input is currently blocked.
    pub fn is_input_blocked(&self) -> bool {
        self.input_blocked
    }

    /// Enter streaming mode: block input and move cursor to end.
    ///
    /// Call this before appending streamed content, then call
    /// [`end_streaming`](Self::end_streaming) when done.
    pub fn begin_streaming(&mut self, cx: &mut Context<Self>) {
        self.streaming_mode = true;
        self.input_blocked = true;
        let end = self.state.buffer.len_bytes();
        self.state.selection = Selection::new(end, end);
        cx.notify();
    }

    /// Exit streaming mode and re-enable user input.
    pub fn end_streaming(&mut self, cx: &mut Context<Self>) {
        self.streaming_mode = false;
        self.input_blocked = false;
        cx.notify();
    }

    /// Returns true if currently in streaming mode.
    pub fn is_streaming(&self) -> bool {
        self.streaming_mode
    }

    /// Returns the current cursor position as a byte offset.
    pub fn cursor_position(&self) -> usize {
        self.state.selection.head
    }

    /// Returns the current selection range, or None if the cursor is collapsed.
    pub fn selection_range(&self) -> Option<std::ops::Range<usize>> {
        if self.state.selection.is_collapsed() {
            None
        } else {
            Some(self.state.selection.range())
        }
    }

    /// Set the cursor position to the given byte offset.
    pub fn set_cursor(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.state.buffer.len_bytes());
        self.state.selection = Selection::new(offset, offset);
        cx.notify();
    }

    /// Move the cursor to the end of the buffer.
    pub fn move_to_end(&mut self, cx: &mut Context<Self>) {
        let end = self.state.buffer.len_bytes();
        self.state.selection = Selection::new(end, end);
        cx.notify();
    }

    /// Move the cursor to the start of the buffer.
    pub fn move_to_start(&mut self, cx: &mut Context<Self>) {
        self.state.selection = Selection::new(0, 0);
        cx.notify();
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

    /// Execute an editor action programmatically.
    ///
    /// This is useful for scripted demos or external control of the editor.
    /// Bypasses `input_blocked` check - use `handle_action` for user input.
    pub fn execute(&mut self, action: &EditorAction, _window: &mut Window, cx: &mut Context<Self>) {
        self.execute_action(action, cx);
    }

    /// Handle an action from GPUI dispatch (respects input_blocked).
    pub fn handle_action(&mut self, action: &EditorAction, cx: &mut Context<Self>) {
        if self.input_blocked {
            // Allow hover updates even when input is blocked
            if let EditorAction::UpdateHover {
                over_checkbox,
                over_link,
                ..
            } = *action
                && (self.hovering_checkbox != over_checkbox
                    || self.hovering_link_region != over_link)
            {
                self.hovering_checkbox = over_checkbox;
                self.hovering_link_region = over_link;
                cx.notify();
            }
            return;
        }
        self.execute_action(action, cx);
    }

    /// Internal action execution (no input_blocked check).
    fn execute_action(&mut self, action: &EditorAction, cx: &mut Context<Self>) {
        self.reset_cursor_blink();
        match action {
            EditorAction::Type(c) => {
                self.insert_text(&c.to_string());
            }
            EditorAction::Enter => {
                self.enter();
            }
            EditorAction::ShiftEnter => {
                self.shift_enter();
            }
            EditorAction::ShiftAltEnter => {
                self.shift_alt_enter();
            }
            EditorAction::Tab => {
                self.tab();
            }
            EditorAction::ShiftTab => {
                self.shift_tab();
            }
            EditorAction::Backspace => {
                self.delete_backward();
            }
            EditorAction::Move(direction) => {
                self.move_in_direction(direction.clone(), false);
            }
            EditorAction::Click {
                offset,
                shift,
                click_count,
            } => {
                self.state.handle_click(*offset, *shift, *click_count);
            }
            EditorAction::Drag { offset } => {
                if !self.in_drag_scroll_zone {
                    self.state.handle_drag(*offset);
                    self.is_selecting = true;
                }
            }
            EditorAction::ToggleCheckbox { line_number } => {
                self.toggle_checkbox(*line_number, cx);
                return; // toggle_checkbox calls cx.notify() itself
            }
            EditorAction::UpdateHover {
                over_checkbox,
                over_link,
                ..
            } => {
                if self.hovering_checkbox != *over_checkbox
                    || self.hovering_link_region != *over_link
                {
                    self.hovering_checkbox = *over_checkbox;
                    self.hovering_link_region = *over_link;
                    cx.notify();
                }
                return; // Only notify if hover state actually changed
            }
            EditorAction::OpenLink { url } => {
                let _ = open::that(url);
                return; // Opening a link doesn't change editor state
            }
        }
        cx.notify();
    }

    fn compute_total_content_height(&self, rem_size: Pixels) -> f32 {
        let total_lines = self.state.buffer.line_count();
        let default_line_h = f32::from(self.config.line_height.to_pixels(rem_size));

        let mut measured_height = 0.0f32;
        let mut measured_count = 0usize;

        for i in 0..total_lines {
            if let Some(bounds) = self.list_state.bounds_for_item(i) {
                measured_height += f32::from(bounds.size.height);
                measured_count += 1;
            }
        }

        let unmeasured = total_lines.saturating_sub(measured_count);
        measured_height + (unmeasured as f32 * default_line_h)
    }

    fn compute_scroll_offset_pixels(&self, rem_size: Pixels) -> f32 {
        let default_line_h = f32::from(self.config.line_height.to_pixels(rem_size));
        let scroll = self.list_state.logical_scroll_top();

        let mut offset = 0.0f32;
        for i in 0..scroll.item_ix {
            if let Some(bounds) = self.list_state.bounds_for_item(i) {
                offset += f32::from(bounds.size.height);
            } else {
                offset += default_line_h;
            }
        }
        offset + f32::from(scroll.offset_in_item)
    }

    fn render_scrollbar(
        &mut self,
        theme: &EditorTheme,
        rem_size: Pixels,
        editor_id: usize,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let viewport = self.list_state.viewport_bounds();
        let viewport_h = f32::from(viewport.size.height);
        let total_h = self.compute_total_content_height(rem_size);

        if total_h <= viewport_h {
            return None;
        }

        let track_h = viewport_h;
        let min_thumb_h = 20.0f32;
        let thumb_h = ((viewport_h / total_h) * track_h).max(min_thumb_h);
        let scroll_offset = self.compute_scroll_offset_pixels(rem_size);
        let thumb_top = if total_h > 0.0 {
            (scroll_offset / total_h) * track_h
        } else {
            0.0
        };
        let thumb_top = thumb_top.min(track_h - thumb_h);

        let track_color = {
            let mut c: Hsla = theme.comment.into();
            c.a = 0.15;
            Rgba::from(c)
        };
        let thumb_color = {
            let mut c: Hsla = theme.comment.into();
            c.a = 0.4;
            Rgba::from(c)
        };
        let thumb_hover_color = {
            let mut c: Hsla = theme.comment.into();
            c.a = 0.6;
            Rgba::from(c)
        };

        let thumb_h_val = thumb_h;
        let thumb_top_val = thumb_top;
        let total_h_val = total_h;
        let track_h_val = track_h;
        let viewport_h_val = viewport_h;

        Some(
            div()
                .id(("scrollbar", editor_id))
                .absolute()
                .right_0()
                .top_0()
                .h_full()
                .w(px(8.0))
                .hover(|d| d.w(px(12.0)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(
                        move |editor, event: &gpui::MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();

                            let click_y = f32::from(event.position.y);
                            editor.scrollbar_drag_start_y = Some(px(click_y));
                            editor.scrollbar_pending_page_turn = true;
                            cx.notify();
                        },
                    ),
                )
                .on_drag(ScrollbarDrag, |_drag, _point, _window, cx| {
                    cx.new(|_| EmptyDragView)
                })
                .on_drag_move(cx.listener(
                    move |editor,
                          event: &DragMoveEvent<ScrollbarDrag>,
                          _window,
                          cx| {
                        let start_y = match editor.scrollbar_drag_start_y {
                            Some(y) => y,
                            None => return,
                        };
                        let mouse_y = event.event.position.y;

                        if editor.scrollbar_pending_page_turn {
                            let first_delta =
                                (f32::from(mouse_y) - f32::from(start_y)).abs();
                            editor.scrollbar_drag_start_y = Some(mouse_y);
                            if first_delta > 3.0 {
                                editor.scrollbar_pending_page_turn = false;
                            }
                            return;
                        }

                        let delta_y_px = f32::from(mouse_y) - f32::from(start_y);

                        let track_range = track_h_val - thumb_h_val;
                        if track_range > 0.0 {
                            let content_delta =
                                (delta_y_px / track_range) * total_h_val;
                            editor.list_state.scroll_by(px(content_delta));
                            editor.scrollbar_drag_start_y = Some(mouse_y);
                            cx.notify();
                        }
                    },
                ))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(
                        move |editor, _event: &gpui::MouseUpEvent, _window, cx| {
                            if editor.scrollbar_pending_page_turn {
                                editor.scrollbar_pending_page_turn = false;
                                if let Some(start_y) = editor.scrollbar_drag_start_y {
                                    let click_y = f32::from(start_y);
                                    let thumb_center = thumb_top_val + thumb_h_val / 2.0;
                                    if click_y < thumb_center {
                                        editor.list_state.scroll_by(px(-viewport_h_val));
                                    } else {
                                        editor.list_state.scroll_by(px(viewport_h_val));
                                    }
                                }
                            }
                            editor.scrollbar_drag_start_y = None;
                            cx.notify();
                        },
                    ),
                )
                .bg(track_color)
                .rounded(px(4.0))
                .cursor(CursorStyle::Arrow)
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(thumb_top))
                        .h(px(thumb_h))
                        .bg(thumb_color)
                        .hover(|d| d.bg(thumb_hover_color))
                        .rounded(px(4.0)),
                )
                .into_any_element(),
        )
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let buffer_version = self.state.buffer.version();
        let content_changed = buffer_version != self.last_synced_version;
        if content_changed {
            self.last_synced_version = buffer_version;
            self.sync_list_state(cx);
        }

        // Update status bar info
        let cursor_offset = self.state.cursor().offset;
        let cursor_line = self.state.buffer.byte_to_line(cursor_offset);
        let line_start = self.state.buffer.line_to_byte(cursor_line);
        let cursor_col = cursor_offset - line_start;
        // Build full nested context by walking up the tree
        let context_markers = self.state.build_nested_context(cursor_offset);
        let heading_level = self.find_current_heading(cursor_line);
        let total_lines = self.state.buffer.line_count();

        let first_visible_line = self.list_state.logical_scroll_top().item_ix;
        // Estimate last visible line by scanning from first visible until out of viewport
        let viewport = self.list_state.viewport_bounds();
        let mut last_visible_line = first_visible_line;
        for i in first_visible_line..total_lines {
            if let Some(bounds) = self.list_state.bounds_for_item(i) {
                if bounds.origin.y <= viewport.origin.y + viewport.size.height {
                    last_visible_line = i;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Detect naked URLs in visible lines â€” only when content changed
        let _naked_urls_by_line = if content_changed {
            self.detect_naked_urls_in_range(first_visible_line, last_visible_line + 1)
        } else {
            HashMap::new()
        };

        // Update autocomplete only when cursor position changed (not on every render)
        let cursor_offset_changed = self.last_cursor_offset != Some(cursor_offset);
        self.last_cursor_offset = Some(cursor_offset);
        if cursor_offset_changed && self.update_autocomplete_from_cursor() {
            self.fetch_autocomplete_suggestions_debounced(cx);
        }

        // Only primary editor updates status bar info
        if self.is_primary {
            self.status_info = StatusBarInfo {
                context_markers,
                heading_level,
                cursor_line: cursor_line + 1, // 1-indexed
                cursor_col: cursor_col + 1,   // 1-indexed
                total_lines,
                first_visible_line,
                last_visible_line,
            };
        }

        let theme = self.config.theme.clone();
        let code_font = font(&self.config.code_font);

        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let measure_run = gpui::TextRun {
            len: 1,
            font: code_font.clone(),
            color: gpui::transparent_black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_line(" ".into(), font_size, &[measure_run], None);
        let monospace_char_width = shaped.width;

        let line_theme = LineTheme {
            text_color: theme.foreground,
            cursor_color: theme.purple,
            link_color: theme.cyan,
            selection_color: theme.selection,
            border_color: theme.comment,
            code_color: theme.pink,
            fence_color: theme.comment,
            fence_lang_color: theme.green,
            checkbox_unchecked_color: theme.orange,
            checkbox_checked_color: theme.green,
            emphasis_color: theme.emphasis,
            text_font: font(&self.config.text_font),
            code_font,
            monospace_char_width,
            line_height: self.config.line_height,
        };

        let user_message_bg: Rgba = {
            let mut c: Hsla = theme.purple.into();
            c.a = 0.05;
            c.into()
        };

        // Only show cursor and selection when this editor is focused and input is not blocked
        let is_focused = self.focus_handle.is_focused(window);
        let is_editing = is_focused && !self.input_blocked;
        // Visual cursor visibility includes blink state (controls whether cursor is drawn)
        let show_cursor_visual = is_editing && self.cursor_blink_visible;
        let cursor_offset = self.state.selection.head;
        let selection_range = if is_editing && !self.state.selection.is_collapsed() {
            Some(self.state.selection.range())
        } else {
            None
        };
        // For editing mode detection (showing/hiding markdown markers), use the real
        // cursor offset when the editor is focused. This prevents flickering caused by
        // the cursor blink timer toggling between editing and rendered modes.
        // Only use usize::MAX when the editor is not focused (rendered/read-only mode).
        let editing_cursor_offset = if is_editing {
            cursor_offset
        } else {
            usize::MAX
        };

        let base_path = self.config.base_path.clone();

        let cursor_line = self.state.buffer.byte_to_line(cursor_offset);
        let cursor_line_changed = self.last_cursor_line != Some(cursor_line);
        self.last_cursor_line = Some(cursor_line);

        // When scroll is requested (e.g., typing) or in streaming mode,
        // check if cursor line is near the bottom edge and scroll.
        if self.scroll_to_cursor_pending || self.streaming_mode {
            self.scroll_to_cursor_pending = false;
            let scroll_buffer = self.config.line_height.to_pixels(window.rem_size());
            if let Some(cursor_bounds) = self.list_state.bounds_for_item(cursor_line) {
                let viewport = self.list_state.viewport_bounds();
                let is_last = cursor_line == self.state.buffer.line_count().saturating_sub(1);
                let cursor_bottom = if is_last {
                    cursor_bounds.origin.y + scroll_buffer
                } else {
                    cursor_bounds.origin.y + cursor_bounds.size.height
                };
                let viewport_bottom = viewport.origin.y + viewport.size.height;
                // Only scroll if cursor is near bottom edge (within buffer zone)
                if cursor_bottom > viewport_bottom - scroll_buffer {
                    self.list_state.scroll_to_reveal_item(cursor_line);
                    self.list_state.scroll_by(scroll_buffer);
                }
            } else {
                self.list_state.scroll_to_reveal_item(cursor_line);
            }
        } else if cursor_line_changed {
            if let Some(cursor_bounds) = self.list_state.bounds_for_item(cursor_line) {
                let viewport = self.list_state.viewport_bounds();
                let cursor_top = cursor_bounds.origin.y;
                let is_last = cursor_line == self.state.buffer.line_count().saturating_sub(1);
                let line_h = self.config.line_height.to_pixels(window.rem_size());
                let cursor_bottom = if is_last {
                    cursor_top + line_h
                } else {
                    cursor_top + cursor_bounds.size.height
                };
                let viewport_top = viewport.origin.y;
                let viewport_bottom = viewport_top + viewport.size.height;

                if cursor_top < viewport_top || cursor_bottom > viewport_bottom {
                    self.list_state.scroll_to_reveal_item(cursor_line);
                }
            } else {
                self.list_state.scroll_to_reveal_item(cursor_line);
            }
        }

        let line_theme_for_list = line_theme.clone();
        let theme_for_highlights = self.config.theme.clone();
        let padding_top = self.config.padding_top;
        let padding_bottom_px = self.config.padding_bottom.to_pixels(window.rem_size());
        let viewport_h = self.list_state.viewport_bounds().size.height;
        let padding_bottom = padding_bottom_px + viewport_h / 2.0;
        let max_line_width = self.config.max_line_width;
        let snapshot = self.state.buffer.render_snapshot();
        let user_message_lines = self.user_message_lines.clone();

        let input_blocked = self.input_blocked;

        let editor_id = self.instance_id;
        let cursor_screen_pos = self.cursor_screen_pos.clone();
        let line_list = div().id(("line-list", editor_id)).size_full().child(
            list(self.list_state.clone(), move |ix, _window, _cx| {
                // Bounds check: ensure line index is valid for this snapshot
                if ix >= snapshot.line_count() {
                    warn!(
                        "[writ] list callback: ix {} >= line_count {}, rope_len {}",
                        ix,
                        snapshot.line_count(),
                        snapshot.rope.len_bytes()
                    );
                    return div().into_any_element();
                }

                // Helper to build a Line element from a snapshot
                let build_line = |snap: &RenderSnapshot,
                                  line_idx: usize,
                                  extra_styles: Vec<StyledRegion>,
                                  line_background: Option<Rgba>,
                                  inline_highlight_ranges: Vec<Range<usize>>,
                                  inline_highlight_color: Option<Rgba>,
                                  block_input: bool,
                                  csp: Option<Rc<RefCell<CursorScreenPosition>>>|
                 -> Line {
                    let line_markers = snap.line_markers(line_idx);
                    let mut inline_styles = snap.inline_styles_for_line(line_idx);
                    inline_styles.extend(extra_styles);
                    inline_styles.sort_by_key(|s| s.full_range.start);

                    let code_highlights: Vec<_> = snap
                        .code_highlights_for_line(line_idx)
                        .iter()
                        .map(|span| {
                            (
                                span.clone(),
                                theme_for_highlights.color_for_highlight(span.highlight_id),
                            )
                        })
                        .collect();

                    Line::new(LineParams {
                        line: line_markers,
                        rope: snap.rope.clone(),
                        cursor_offset: if block_input {
                            usize::MAX
                        } else {
                            editing_cursor_offset
                        },
                        inline_styles,
                        theme: line_theme_for_list.clone(),
                        selection_range: if block_input {
                            None
                        } else {
                            selection_range.clone()
                        },
                        code_highlights,
                        base_path: base_path.clone(),
                        github_ref_ranges: Vec::new(),
                        hovered_ref_range: None,
                        input_blocked: block_input || input_blocked,
                        max_line_width,
                        line_background,
                        inline_highlight_ranges,
                        inline_highlight_color,
                        show_cursor: if block_input {
                            false
                        } else {
                            show_cursor_visual
                        },
                        cursor_screen_pos: csp,
                    })
                };

                let extra_styles = Vec::new();

                // Determine line background and inline highlight colors
                let is_user_message = user_message_lines.iter().any(|r| r.contains(&ix));

                let line_bg = if is_user_message {
                    Some(user_message_bg)
                } else {
                    None
                };

                let inline_highlight_ranges: Vec<Range<usize>> = Vec::new();
                let inline_highlight_color = None;

                // Build the main line element
                let line_element = build_line(
                    &snapshot,
                    ix,
                    extra_styles,
                    line_bg,
                    inline_highlight_ranges,
                    inline_highlight_color,
                    false, // don't block input for main lines
                    Some(cursor_screen_pos.clone()),
                );

                // Add top padding to first line, bottom padding to last line
                let is_first = ix == 0;
                let is_last = ix == snapshot.line_count().saturating_sub(1);
                div()
                    .when(is_first, |d| d.pt(padding_top))
                    .when(is_last, |d| d.pb(padding_bottom))
                    .child(line_element)
                    .into_any_element()
            })
            .size_full(),
        );

        div()
            .id(("editor", editor_id))
            .track_focus(&self.focus_handle)
            .key_context("Editor")
            .on_key_down(cx.listener(Self::on_key_down))
            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
            .on_action(cx.listener(
                |editor: &mut Editor, action: &DispatchEditorAction, _window, cx| {
                    editor.handle_action(&action.0, cx);
                },
            ))
            .on_action(cx.listener(
                |_editor: &mut Editor, _: &crate::file_ops::Save, window, cx| {
                    crate::file_ops::set_dialog_open(true);
                    let entity = cx.entity().clone();
                    window.defer(cx, move |_window, cx| {
                        entity.update(cx, |editor, cx| {
                            editor.save(cx);
                            crate::file_ops::set_dialog_open(false);
                        });
                    });
                },
            ))
            .on_action(cx.listener(
                |_editor: &mut Editor, _: &crate::file_ops::SaveAs, window, cx| {
                    crate::file_ops::set_dialog_open(true);
                    let entity = cx.entity().clone();
                    window.defer(cx, move |_window, cx| {
                        entity.update(cx, |editor, cx| {
                            editor.save_as(cx);
                            crate::file_ops::set_dialog_open(false);
                        });
                    });
                },
            ))
            .on_action(cx.listener(
                |_editor: &mut Editor, _: &crate::file_ops::OpenFile, window, cx| {
                    crate::file_ops::set_dialog_open(true);
                    let entity = cx.entity().clone();
                    window.defer(cx, move |_window, cx| {
                        entity.update(cx, |editor, cx| {
                            editor.open_file(cx);
                            crate::file_ops::set_dialog_open(false);
                        });
                    });
                },
            ))
            .on_action(cx.listener(
                |_editor: &mut Editor, _: &crate::file_ops::NewFile, window, cx| {
                    crate::file_ops::set_dialog_open(true);
                    let entity = cx.entity().clone();
                    window.defer(cx, move |_window, cx| {
                        entity.update(cx, |editor, cx| {
                            editor.new_file(cx);
                            crate::file_ops::set_dialog_open(false);
                        });
                    });
                },
            ))
            .on_action(cx.listener(
                |editor: &mut Editor, _: &CenterLine, window, cx| {
                    if !KeyMode::is_mac(cx) {
                        return;
                    }
                    let cursor = editor.state.selection.head;
                    let line_idx = editor.state.buffer.byte_to_line(cursor);

                    // Case A: line is visible and measured â†’ center immediately
                    if let Some(item_bounds) = editor.list_state.bounds_for_item(line_idx) {
                        let viewport = editor.list_state.viewport_bounds();
                        let viewport_center_y = viewport.origin.y + viewport.size.height / 2.0;
                        let item_center_y = item_bounds.origin.y + item_bounds.size.height / 2.0;
                        let offset = item_center_y - viewport_center_y;

                        if offset != px(0.0) {
                            editor.list_state.scroll_by(offset);
                            cx.notify();
                        }
                        return;
                    }

                    // Case B: line not measured â€” reveal it, then center after layout
                    editor.list_state.scroll_to_reveal_item(line_idx);
                    let entity = cx.entity().clone();
                    window.defer(cx, move |_window, cx| {
                        entity.update(cx, |editor, cx| {
                            let Some(item_bounds) = editor.list_state.bounds_for_item(line_idx) else {
                                return;
                            };
                            let viewport = editor.list_state.viewport_bounds();
                            let viewport_center_y = viewport.origin.y + viewport.size.height / 2.0;
                            let item_center_y = item_bounds.origin.y + item_bounds.size.height / 2.0;
                            let offset = item_center_y - viewport_center_y;

                            if offset != px(0.0) {
                                editor.list_state.scroll_by(offset);
                                cx.notify();
                            }
                        });
                    });
                },
            ))
            // IMPORTANT: Use capture phase to focus this editor BEFORE child elements
            // (Line components) handle mouse events. This ensures DispatchEditorAction
            // from Line click handlers will be routed to THIS editor.
            // Don't focus if input is blocked (read-only mode).
            .capture_any_mouse_down(cx.listener(
                |editor, _event: &gpui::MouseDownEvent, window, _cx| {
                    if !editor.input_blocked {
                        editor.focus_handle.focus(window);
                    }
                },
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|editor, event: &gpui::MouseDownEvent, window, cx| {
                    // Focus already handled in capture phase above

                    if editor.input_blocked {
                        return;
                    }
                    // Only handle if not already handled by a line element
                    // (lines call prevent_default but don't stop propagation to allow on_drag)
                    if window.default_prevented() {
                        return;
                    }
                    // Check if click is below the last line (empty space at bottom)
                    // Only then do we jump cursor to end of buffer
                    let line_count = editor.state.buffer.line_count();
                    if line_count > 0 {
                        if let Some(last_line_bounds) =
                            editor.list_state.bounds_for_item(line_count - 1)
                        {
                            let last_line_bottom =
                                last_line_bounds.origin.y + last_line_bounds.size.height;
                            if event.position.y <= last_line_bottom {
                                // Click is in side margins at height of existing content - ignore
                                return;
                            }
                        } else {
                            // Last line not visible/measured - ignore click
                            return;
                        }
                    }
                    // Click is in empty space below content
                    let end = editor.state.buffer.len_bytes();
                    editor.state.selection = Selection::new(end, end);
                    editor.request_scroll_to_cursor();
                    window.refresh();
                    cx.notify();
                }),
            )
            .on_drag(SelectionDrag, |_drag, _point, _window, cx| {
                // Return an empty view - we don't need a visible drag indicator
                cx.new(|_| EmptyDragView)
            })
            .on_drag_move(cx.listener(
                |editor, event: &DragMoveEvent<SelectionDrag>, window, cx| {
                    use std::time::{Duration, Instant};

                    // When dragging near viewport edges, move cursor to trigger auto-scroll
                    let viewport = editor.list_state.viewport_bounds();
                    let mouse_y = event.event.position.y;

                    // Get window bounds to handle maximized windows
                    let window_bounds = window.bounds();

                    // Create "hot zones" at the edges that trigger scrolling
                    // Zone size is one line height - scrolling triggers when mouse enters
                    // this margin or goes past the viewport entirely
                    let zone_size = editor.config.line_height.to_pixels(window.rem_size());

                    // For top: use viewport top (content starts there)
                    let top_threshold = viewport.origin.y + zone_size;

                    // For bottom: use the smaller of viewport bottom or window bottom
                    // This handles maximized windows where viewport == window
                    let viewport_bottom = viewport.origin.y + viewport.size.height;
                    let window_bottom = window_bounds.origin.y + window_bounds.size.height;
                    let effective_bottom = viewport_bottom.min(window_bottom);
                    let bottom_threshold = effective_bottom - zone_size;

                    // Calculate distance outside the inset bounds and direction
                    let (delta, direction): (f32, i32) = if mouse_y < top_threshold {
                        ((top_threshold - mouse_y).into(), -1) // up
                    } else if mouse_y > bottom_threshold {
                        ((mouse_y - bottom_threshold).into(), 1) // down
                    } else {
                        // Mouse is inside safe zone - reset throttle and allow line's on_drag
                        editor.last_drag_scroll = None;
                        editor.in_drag_scroll_zone = false;
                        return;
                    };

                    // We're in the scroll zone - prevent line's on_drag from resetting selection
                    editor.in_drag_scroll_zone = true;

                    // Throttle inversely proportional to distance
                    // Close to edge: ~30ms, far from edge: ~8ms
                    let speed_factor = (delta.powf(1.2) / 50.0).clamp(0.5, 6.0);
                    let throttle_ms = (30.0 / speed_factor) as u64;
                    let throttle = Duration::from_millis(throttle_ms.clamp(8, 50));

                    let now = Instant::now();
                    if let Some(last) = editor.last_drag_scroll
                        && now.duration_since(last) < throttle
                    {
                        return;
                    }
                    editor.last_drag_scroll = Some(now);

                    // Scroll by one line height in the appropriate direction
                    // Using scroll_by instead of scroll_to_reveal_item gives smoother
                    // scrolling through wrapped lines (doesn't jump entire item)
                    let scroll_amount = if direction < 0 { -zone_size } else { zone_size };
                    editor.list_state.scroll_by(scroll_amount);

                    // Move cursor one line in the appropriate direction
                    let cursor = editor.state.selection.cursor();
                    let new_cursor = if direction < 0 {
                        cursor.move_up(&editor.state.buffer)
                    } else {
                        cursor.move_down(&editor.state.buffer)
                    };
                    editor.state.selection = editor.state.selection.extend_to(new_cursor.offset);
                    cx.notify();
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|editor, _event: &gpui::MouseUpEvent, _window, cx| {
                    let mut changed = false;
                    if editor.is_selecting {
                        editor.is_selecting = false;
                        changed = true;
                    }
                    if editor.scrollbar_drag_start_y.is_some() {
                        editor.scrollbar_drag_start_y = None;
                        editor.scrollbar_pending_page_turn = false;
                        changed = true;
                    }
                    if changed {
                        cx.notify();
                    }
                }),
            )
            .size_full()
            .px(self.config.padding_x)
            .font(line_theme.text_font.clone())
            .text_color(line_theme.text_color)
            .cursor(
                if self.hovering_checkbox || (self.hovering_link_region && self.ctrl_held) {
                    CursorStyle::PointingHand
                } else {
                    CursorStyle::IBeam
                },
            )
            .child(line_list)
            .children(self.render_scrollbar(&theme, window.rem_size(), editor_id, cx))
            .children(self.render_autocomplete(&line_theme, window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trim leading newline from raw string literals for readability.
    /// Allows writing:
    /// ```
    /// r#"
    /// - item one
    /// - item two
    /// "#
    /// ```
    fn trim_raw(s: &str) -> &str {
        s.strip_prefix('\n').unwrap_or(s)
    }

    /// Helper to create an EditorState with cursor at a specific position.
    /// The cursor position is indicated by | in the input string.
    fn editor_with_cursor(input: &str) -> EditorState {
        let input = trim_raw(input);
        let cursor_pos = input
            .find('|')
            .expect("Input must contain | for cursor position");
        let content = input.replace('|', "");
        let mut state = EditorState::new(&content);
        state.set_cursor(cursor_pos);
        state
    }

    /// Helper to check editor state matches expected content with cursor.
    fn assert_editor_eq(state: &EditorState, expected: &str) {
        let expected = trim_raw(expected);
        let text = state.text();
        let cursor = state.cursor().offset;
        let mut actual = String::new();
        actual.push_str(&text[..cursor]);
        actual.push('|');
        actual.push_str(&text[cursor..]);
        assert_eq!(actual, expected);
    }

    /// Helper to check editor state with selection.
    /// Format: `<` marks start of selection, `|` marks head (cursor), `>` marks end.
    /// Examples:
    ///   - `|hello` - cursor at start, no selection
    ///   - `<hello|>` - "hello" selected, cursor at end
    ///   - `<|hello>` - "hello" selected, cursor at start
    fn assert_selection_eq(state: &EditorState, expected: &str) {
        let expected = trim_raw(expected);
        let text = state.text();
        let selection = &state.selection;

        let anchor = selection.anchor;
        let head = selection.head;
        let start = anchor.min(head);
        let end = anchor.max(head);
        let is_collapsed = anchor == head;

        let mut actual = String::new();
        let mut byte_pos = 0;

        for c in text.chars() {
            if !is_collapsed && byte_pos == start {
                actual.push('<');
            }
            if byte_pos == head {
                actual.push('|');
            }
            if !is_collapsed && byte_pos == end {
                actual.push('>');
            }
            actual.push(c);
            byte_pos += c.len_utf8();
        }

        // Handle markers at end of text
        if !is_collapsed && byte_pos == start {
            actual.push('<');
        }
        if byte_pos == head {
            actual.push('|');
        }
        if !is_collapsed && byte_pos == end {
            actual.push('>');
        }

        assert_eq!(actual, expected, "Selection mismatch");
    }

    mod click_tests {
        use super::*;

        #[test]
        fn click_sets_cursor() {
            let mut state = editor_with_cursor("hello| world");
            state.handle_click(0, false, 1);
            assert_editor_eq(&state, "|hello world");
        }

        #[test]
        fn click_middle() {
            let mut state = editor_with_cursor("|hello world");
            state.handle_click(6, false, 1);
            assert_editor_eq(&state, "hello |world");
        }

        #[test]
        fn shift_click_extends_selection() {
            let mut state = editor_with_cursor("hello| world");
            state.handle_click(11, true, 1);
            assert_selection_eq(&state, "hello< world|>");
        }

        #[test]
        fn shift_click_backward() {
            let mut state = editor_with_cursor("hello| world");
            state.handle_click(0, true, 1);
            assert_selection_eq(&state, "<|hello> world");
        }

        #[test]
        fn double_click_selects_word() {
            let mut state = editor_with_cursor("|hello world");
            state.handle_click(2, false, 2);
            assert_selection_eq(&state, "<hello|> world");
        }

        #[test]
        fn double_click_second_word() {
            let mut state = editor_with_cursor("|hello world");
            state.handle_click(8, false, 2);
            assert_selection_eq(&state, "hello <world|>");
        }

        #[test]
        fn triple_click_selects_line() {
            let mut state = editor_with_cursor("|hello world");
            state.handle_click(2, false, 3);
            assert_selection_eq(&state, "<hello world|>");
        }

        #[test]
        fn drag_extends_selection() {
            let mut state = editor_with_cursor("|hello world");
            state.handle_click(0, false, 1);
            state.handle_drag(5);
            assert_selection_eq(&state, "<hello|> world");
        }

        #[test]
        fn drag_backward() {
            let mut state = editor_with_cursor("hello world|");
            state.handle_click(11, false, 1);
            state.handle_drag(6);
            assert_selection_eq(&state, "hello <|world>");
        }
    }

    mod cursor_movement_tests {
        use super::*;

        #[test]
        fn move_left() {
            let mut state = editor_with_cursor("hel|lo");
            state.move_left();
            assert_editor_eq(&state, "he|llo");
        }

        #[test]
        fn move_left_at_start() {
            let mut state = editor_with_cursor("|hello");
            state.move_left();
            assert_editor_eq(&state, "|hello");
        }

        #[test]
        fn move_right() {
            let mut state = editor_with_cursor("he|llo");
            state.move_right();
            assert_editor_eq(&state, "hel|lo");
        }

        #[test]
        fn move_right_at_end() {
            let mut state = editor_with_cursor("hello|");
            state.move_right();
            assert_editor_eq(&state, "hello|");
        }

        #[test]
        fn move_up() {
            let mut state = editor_with_cursor("line one\nline |two\nline three");
            state.move_up();
            assert_editor_eq(&state, "line |one\nline two\nline three");
        }

        #[test]
        fn move_up_from_first_line() {
            let mut state = editor_with_cursor("hel|lo\nworld");
            state.move_up();
            assert_editor_eq(&state, "|hello\nworld");
        }

        #[test]
        fn move_down() {
            let mut state = editor_with_cursor("line |one\nline two\nline three");
            state.move_down();
            assert_editor_eq(&state, "line one\nline |two\nline three");
        }

        #[test]
        fn move_down_from_last_line() {
            let mut state = editor_with_cursor("hello\nwor|ld");
            state.move_down();
            assert_editor_eq(&state, "hello\nworld|");
        }

        #[test]
        fn move_up_preserves_column() {
            let mut state = editor_with_cursor("short\nlonger line|");
            state.move_up();
            assert_editor_eq(&state, "short|\nlonger line");
        }

        #[test]
        fn move_to_line_start() {
            let mut state = editor_with_cursor("hello\nwor|ld");
            state.move_to_line_start();
            assert_editor_eq(&state, "hello\n|world");
        }

        #[test]
        fn move_to_line_end() {
            let mut state = editor_with_cursor("hello\nwor|ld");
            state.move_to_line_end();
            assert_editor_eq(&state, "hello\nworld|");
        }
    }

    // ========================================================================
    // New "raw markdown" behavior tests
    // These test the simplified, non-controlling editing paradigm.
    // ========================================================================

    mod raw_enter_tests {
        use super::*;

        // --- Enter: always raw \n ---

        #[test]
        fn enter_on_paragraph_inserts_newline() {
            let mut state = editor_with_cursor("Hello world|");
            state.enter();
            assert_editor_eq(&state, "Hello world\n|");
        }

        #[test]
        fn enter_on_heading_inserts_newline() {
            let mut state = editor_with_cursor("# Hello|");
            state.enter();
            assert_editor_eq(&state, "# Hello\n|");
        }

        #[test]
        fn enter_on_list_item_inserts_newline_no_marker() {
            let mut state = editor_with_cursor("- item one|");
            state.enter();
            assert_editor_eq(&state, "- item one\n|");
        }

        #[test]
        fn enter_on_blockquote_inserts_newline_no_marker() {
            let mut state = editor_with_cursor("> quote|");
            state.enter();
            assert_editor_eq(&state, "> quote\n|");
        }

        #[test]
        fn enter_on_nested_container_inserts_newline_no_markers() {
            let mut state = editor_with_cursor("> - item|");
            state.enter();
            assert_editor_eq(&state, "> - item\n|");
        }

        #[test]
        fn enter_on_empty_list_item_inserts_newline_keeps_marker() {
            let mut state = editor_with_cursor("- item one\n- |");
            state.enter();
            assert_editor_eq(&state, "- item one\n- \n|");
        }

        #[test]
        fn enter_on_empty_blockquote_inserts_newline_keeps_marker() {
            let mut state = editor_with_cursor("> quote one\n> |");
            state.enter();
            assert_editor_eq(&state, "> quote one\n> \n|");
        }

        #[test]
        fn enter_in_code_block_inserts_newline() {
            let mut state = editor_with_cursor("```rust\nlet x = 1;|");
            state.enter();
            assert_editor_eq(&state, "```rust\nlet x = 1;\n|");
        }

        #[test]
        fn enter_on_code_fence_inserts_newline() {
            let mut state = editor_with_cursor("```rust|");
            state.enter();
            assert_editor_eq(&state, "```rust\n|");
        }

        #[test]
        fn enter_preserves_soft_wrap_style() {
            // Adjacent lines without blank line between them
            let mut state = editor_with_cursor("First sentence.\nSecond sentence.|");
            state.enter();
            assert_editor_eq(&state, "First sentence.\nSecond sentence.\n|");
        }

        // --- Shift+Enter: continue container ---

        #[test]
        fn shift_enter_on_list_item_continues_list() {
            let mut state = editor_with_cursor("- item one|");
            state.shift_enter();
            assert_editor_eq(&state, "- item one\n- |");
        }

        #[test]
        fn shift_enter_on_blockquote_continues_blockquote() {
            let mut state = editor_with_cursor("> quote|");
            state.shift_enter();
            assert_editor_eq(&state, "> quote\n> |");
        }

        #[test]
        fn shift_enter_on_nested_container_continues_all() {
            let mut state = editor_with_cursor("> - item|");
            state.shift_enter();
            assert_editor_eq(&state, "> - item\n> - |");
        }

        #[test]
        fn shift_enter_on_paragraph_just_inserts_newline() {
            let mut state = editor_with_cursor("Hello world|");
            state.shift_enter();
            assert_editor_eq(&state, "Hello world\n|");
        }

        #[test]
        fn shift_enter_on_heading_just_inserts_newline() {
            let mut state = editor_with_cursor("# Hello|");
            state.shift_enter();
            assert_editor_eq(&state, "# Hello\n|");
        }

        // --- Shift+Alt+Enter: indented continuation ---

        #[test]
        fn shift_alt_enter_on_list_item_creates_indent() {
            let mut state = editor_with_cursor("- item one|");
            state.shift_alt_enter();
            assert_editor_eq(&state, "- item one\n  |");
        }

        #[test]
        fn shift_alt_enter_on_blockquote_creates_indent_outside() {
            let mut state = editor_with_cursor("> quote|");
            state.shift_alt_enter();
            assert_editor_eq(&state, "> quote\n  |");
        }

        #[test]
        fn shift_alt_enter_on_nested_container_creates_indent_inside() {
            let mut state = editor_with_cursor("> - item|");
            state.shift_alt_enter();
            assert_editor_eq(&state, "> - item\n>   |");
        }

        #[test]
        fn shift_alt_enter_on_paragraph_just_inserts_newline() {
            let mut state = editor_with_cursor("Hello world|");
            state.shift_alt_enter();
            assert_editor_eq(&state, "Hello world\n|");
        }
    }

    mod raw_backspace_tests {
        use super::*;

        #[test]
        fn backspace_deletes_char() {
            let mut state = editor_with_cursor("hello|");
            state.delete_backward();
            assert_editor_eq(&state, "hell|");
        }

        #[test]
        fn backspace_at_line_start_joins_lines() {
            let mut state = editor_with_cursor("line one\n|line two");
            state.delete_backward();
            assert_editor_eq(&state, "line one|line two");
        }

        #[test]
        fn backspace_deletes_entire_list_marker() {
            let mut state = editor_with_cursor("- |");
            state.delete_backward();
            assert_editor_eq(&state, "|");
        }

        #[test]
        fn backspace_deletes_innermost_marker_first() {
            let mut state = editor_with_cursor("> - |");
            state.delete_backward();
            assert_editor_eq(&state, "> |");
        }

        #[test]
        fn backspace_then_deletes_outer_marker() {
            let mut state = editor_with_cursor("> |");
            state.delete_backward();
            assert_editor_eq(&state, "|");
        }

        #[test]
        fn backspace_deletes_entire_indent() {
            // Indent after list item is atomic - need context for tree-sitter to recognize it
            let mut state = editor_with_cursor("- item\n  |text");
            state.delete_backward();
            assert_editor_eq(&state, "- item\n|text");
        }

        #[test]
        fn backspace_in_middle_of_text_deletes_char() {
            let mut state = editor_with_cursor("- item o|ne");
            state.delete_backward();
            assert_editor_eq(&state, "- item |ne");
        }

        #[test]
        fn backspace_on_empty_line_after_list_joins() {
            let mut state = editor_with_cursor("- item one\n|");
            state.delete_backward();
            assert_editor_eq(&state, "- item one|");
        }

        #[test]
        fn backspace_sequence_through_markers_and_join() {
            // Start: "- item one\n- |"
            // Backspace 1: delete "- " marker -> "- item one\n|"
            // Backspace 2: join lines -> "- item one|"
            let mut state = editor_with_cursor("- item one\n- |");
            state.delete_backward();
            assert_editor_eq(&state, "- item one\n|");
            state.delete_backward();
            assert_editor_eq(&state, "- item one|");
        }

        #[test]
        fn backspace_with_content_after_cursor_deletes_marker() {
            let mut state = editor_with_cursor("- |two");
            state.delete_backward();
            assert_editor_eq(&state, "|two");
        }

        #[test]
        fn backspace_deletes_entire_task_list_marker() {
            // Task list now has separate Checkbox and ListItem markers
            // First backspace deletes the checkbox, second deletes the list marker
            let mut state = editor_with_cursor("- [ ] |");
            state.delete_backward();
            assert_editor_eq(&state, "- |");
            state.delete_backward();
            assert_editor_eq(&state, "|");
        }

        #[test]
        fn backspace_deletes_checked_task_list_marker() {
            let mut state = editor_with_cursor("- [x] |");
            state.delete_backward();
            assert_editor_eq(&state, "- |");
            state.delete_backward();
            assert_editor_eq(&state, "|");
        }
    }

    mod raw_tab_tests {
        use super::*;

        // --- Tab cycling through states ---
        // Tree-based: cycle is marker â†’ (para indent if blank) â†’ nested marker â†’ empty

        #[test]
        fn tab_on_empty_line_after_list_adds_marker() {
            // Blank line cycle: ["- ", "  ", "  - ", ""]
            let mut state = editor_with_cursor("- item\n|");
            state.tab();
            assert_editor_eq(&state, "- item\n- |");
        }

        #[test]
        fn tab_twice_after_list_adds_nested_marker() {
            // Cycle is: "" -> "- " -> "  " -> "  - " -> ""
            let mut state = editor_with_cursor("- item\n|");
            state.tab();
            state.tab();
            assert_editor_eq(&state, "- item\n  |"); // para indent
            state.tab();
            assert_editor_eq(&state, "- item\n  - |"); // nested marker
        }

        #[test]
        fn tab_three_times_cycles_back() {
            // Cycle is: "" -> "- " -> "  " -> "  - " -> "" (4 states)
            let mut state = editor_with_cursor("- item\n|");
            state.tab();
            state.tab();
            state.tab();
            state.tab();
            assert_editor_eq(&state, "- item\n|");
        }

        #[test]
        fn tab_cycles_ordered_list_after_checkbox() {
            // Bug case: ordered list preceded by checkbox content
            // Cycle should be: "" -> "2. " -> "   " -> "   1. " -> "" (4 states)
            let mut state = editor_with_cursor("## Writ\n- [ ] item\n\n1. hey\n|");

            state.tab();
            assert_editor_eq(&state, "## Writ\n- [ ] item\n\n1. hey\n2. |");

            state.tab();
            assert_editor_eq(&state, "## Writ\n- [ ] item\n\n1. hey\n   |"); // para indent

            state.tab();
            assert_editor_eq(&state, "## Writ\n- [ ] item\n\n1. hey\n   1. |");

            state.tab();
            assert_editor_eq(&state, "## Writ\n- [ ] item\n\n1. hey\n|");
        }

        #[test]
        fn tab_indents_line_with_content() {
            // Tab should cycle the prefix even when there's content after it
            // Content is preserved and cursor stays in place relative to content
            let mut state = editor_with_cursor("1. hey\n2. asdf|");
            state.tab();
            assert_editor_eq(&state, "1. hey\n   asdf|"); // para indent, content preserved
            state.tab();
            assert_editor_eq(&state, "1. hey\n   1. asdf|"); // nested, content preserved
        }

        #[test]
        fn tab_preserves_unchecked_checkbox_state() {
            // Tab cycling preserves the current line's checkbox state
            // Propagation doesn't happen because tree-sitter can't parse incomplete lines
            // Cycle: "" -> "- [ ] " -> "  " -> "  - [ ] " -> ""
            let mut state = editor_with_cursor("- [x] hey\n- [ ] |");
            state.tab();
            // Checkbox stays unchecked (from current line), no propagation
            assert_editor_eq(&state, "- [x] hey\n  |"); // para indent
            state.tab();
            assert_editor_eq(&state, "- [x] hey\n  - [ ] |"); // nested
            state.tab();
            assert_editor_eq(&state, "- [x] hey\n|");
            state.tab();
            assert_editor_eq(&state, "- [x] hey\n- [ ] |");
        }

        #[test]
        fn tab_preserves_checked_checkbox_state() {
            // Tab cycling preserves the current line's checkbox state
            // Cycle: "" -> "- [x] " -> "  " -> "  - [x] " -> ""
            let mut state = editor_with_cursor("- [ ] hey\n- [x] |");
            state.tab();
            // Checkbox stays checked (from current line), no propagation
            assert_editor_eq(&state, "- [ ] hey\n  |"); // para indent
            state.tab();
            assert_editor_eq(&state, "- [ ] hey\n  - [x] |"); // nested
            state.tab();
            assert_editor_eq(&state, "- [ ] hey\n|");
            state.tab();
            assert_editor_eq(&state, "- [ ] hey\n- [x] |");
        }

        #[test]
        fn tab_new_checkbox_defaults_unchecked() {
            // Starting from empty line, new checkboxes default to unchecked
            // Cycle: "" -> "- [ ] " -> "  " -> "  - [ ] " -> ""
            let mut state = editor_with_cursor("- [x] ~~hey~~\n|");
            state.tab(); // sibling: - [ ] |
            assert_editor_eq(&state, "- [x] ~~hey~~\n- [ ] |");
            state.tab(); // para indent
            assert_editor_eq(&state, "- [x] ~~hey~~\n  |");
            state.tab(); // nested: - [ ] |
            assert_editor_eq(&state, "- [x] ~~hey~~\n  - [ ] |");
        }

        #[test]
        fn typing_after_tab_propagates_checkbox() {
            // Tab creates incomplete line "- [ ] |" which tree-sitter can't parse.
            // Once we type content, tree-sitter recognizes it and propagation happens.
            // Cycle: "" -> "- [ ] " -> "  " -> "  - [ ] " -> ""
            let mut state = editor_with_cursor("- [x] hey\n|");
            state.tab(); // "- [ ] |" - incomplete, no propagation yet
            assert_editor_eq(&state, "- [x] hey\n- [ ] |");
            state.tab(); // para indent
            assert_editor_eq(&state, "- [x] hey\n  |");
            state.tab(); // nest it: "  - [ ] |"
            assert_editor_eq(&state, "- [x] hey\n  - [ ] |");
            // Type a character - now tree-sitter can parse, propagation unchecks parent
            state.insert_text("a");
            assert_editor_eq(&state, "- [ ] hey\n  - [ ] a|");
        }

        #[test]
        fn delete_backward_propagates_checkbox() {
            // Deleting content can affect checkbox propagation
            let mut state = editor_with_cursor("- [x] hey\n  - [ ] ab|");
            // Delete 'b' - still has content, propagation runs (parent stays unchecked)
            state.delete_backward();
            assert_editor_eq(&state, "- [ ] hey\n  - [ ] a|");
        }

        #[test]
        fn delete_forward_propagates_checkbox() {
            // Deleting content forward can affect checkbox propagation
            let mut state = editor_with_cursor("- [x] hey\n  - [ ] |ab");
            // Delete 'a' - still has content, propagation runs
            state.delete_forward();
            assert_editor_eq(&state, "- [ ] hey\n  - [ ] |b");
        }

        #[test]
        fn delete_checkbox_marker_rechecks_parent() {
            // Start with checked parent and one checked nested child
            // Cycle: "" -> "- [ ] " -> "  " -> "  - [ ] " -> ""
            let mut state = editor_with_cursor("- [x] ~~parent~~\n  - [x] ~~nested~~\n|");
            // Tab three times to create a new nested unchecked checkbox (with para indent now in cycle)
            state.tab();
            state.tab();
            state.tab();
            assert_editor_eq(&state, "- [x] ~~parent~~\n  - [x] ~~nested~~\n  - [ ] |");
            // Type to make it parseable - this should uncheck the parent
            state.insert_text("new");
            assert_editor_eq(&state, "- [ ] parent\n  - [x] ~~nested~~\n  - [ ] new|");
            // Now delete backwards to remove the unchecked child entirely
            // First delete the content
            state.delete_backward();
            state.delete_backward();
            state.delete_backward();
            assert_editor_eq(&state, "- [ ] parent\n  - [x] ~~nested~~\n  - [ ] |");
            // Delete the checkbox marker
            state.delete_backward();
            assert_editor_eq(&state, "- [ ] parent\n  - [x] ~~nested~~\n  - |");
            // Delete the list marker
            state.delete_backward();
            assert_editor_eq(&state, "- [x] ~~parent~~\n  - [x] ~~nested~~\n  |");
        }

        #[test]
        fn tab_with_blank_line_between_still_works() {
            // Tree-sitter includes blank lines in list_item
            let mut state = editor_with_cursor("- item\n\n|");
            state.tab();
            assert_editor_eq(&state, "- item\n\n- |");
        }

        #[test]
        fn tab_with_two_blank_lines_still_works() {
            // Tree-sitter includes multiple blank lines in list_item
            let mut state = editor_with_cursor("- item\n\n\n|");
            state.tab();
            assert_editor_eq(&state, "- item\n\n\n- |");
        }

        #[test]
        fn tab_on_blockquote_context_adds_marker() {
            let mut state = editor_with_cursor("> quote\n|");
            state.tab();
            assert_editor_eq(&state, "> quote\n> |");
        }

        #[test]
        fn tab_twice_on_blockquote_context_cycles_back() {
            let mut state = editor_with_cursor("> quote\n|");
            state.tab();
            state.tab();
            assert_editor_eq(&state, "> quote\n|");
        }

        #[test]
        fn tab_on_nested_context_cycles() {
            // Cycle: ["> ", "> - ", ">   ", ">   - ", ""]
            let mut state = editor_with_cursor("> - item\n|");

            state.tab();
            assert_editor_eq(&state, "> - item\n> |");

            state.tab();
            assert_editor_eq(&state, "> - item\n> - |");

            state.tab();
            assert_editor_eq(&state, "> - item\n>   |"); // para indent

            state.tab();
            assert_editor_eq(&state, "> - item\n>   - |");

            state.tab();
            assert_editor_eq(&state, "> - item\n|");
        }

        // --- Shift+Tab cycling backwards ---

        #[test]
        fn shift_tab_cycles_backwards() {
            // Cycle: ["- ", "  ", "  - ", ""]
            // Backwards from "" goes to "  - "
            let mut state = editor_with_cursor("- item\n|");
            state.shift_tab();
            assert_editor_eq(&state, "- item\n  - |");
        }

        #[test]
        fn shift_tab_from_marker_goes_to_empty() {
            let mut state = editor_with_cursor("- item\n- |");
            state.shift_tab();
            assert_editor_eq(&state, "- item\n|");
        }

        #[test]
        fn shift_tab_from_nested_marker_goes_to_marker() {
            // "  - " is nested list, cycle found via ERROR handling
            // Cycle backwards: "  - " -> "  " -> "- " -> ""
            let mut state = editor_with_cursor("- item\n  - |");
            state.shift_tab();
            assert_editor_eq(&state, "- item\n  |"); // para indent
            state.shift_tab();
            assert_editor_eq(&state, "- item\n- |");
        }

        #[test]
        fn tab_after_blank_line_includes_para_indent() {
            // With blank line, para indent should be in cycle
            // Cycle: ["- ", "  ", "  - ", "    ", "    - ", ""]
            let mut state = editor_with_cursor("- parent\n  - nested\n\n|");

            state.tab();
            assert_editor_eq(&state, "- parent\n  - nested\n\n- |");

            state.tab();
            assert_editor_eq(&state, "- parent\n  - nested\n\n  |"); // para indent

            state.tab();
            assert_editor_eq(&state, "- parent\n  - nested\n\n  - |");

            state.tab();
            assert_editor_eq(&state, "- parent\n  - nested\n\n    |"); // nested para indent

            state.tab();
            assert_editor_eq(&state, "- parent\n  - nested\n\n    - |");

            state.tab();
            assert_editor_eq(&state, "- parent\n  - nested\n\n|"); // back to empty
        }

        #[test]
        fn tab_no_blank_line_includes_para_indent() {
            // Para indent is now always in cycle, even without blank line
            // Cycle: ["- ", "  ", "  - ", "    ", "    - ", ""]
            let mut state = editor_with_cursor("- parent item\n  - nested with tab\n|");

            state.tab();
            assert_editor_eq(&state, "- parent item\n  - nested with tab\n- |");

            state.tab();
            assert_editor_eq(&state, "- parent item\n  - nested with tab\n  |"); // para indent

            state.tab();
            assert_editor_eq(&state, "- parent item\n  - nested with tab\n  - |");

            state.tab();
            assert_editor_eq(&state, "- parent item\n  - nested with tab\n    |"); // nested para indent

            state.tab();
            assert_editor_eq(&state, "- parent item\n  - nested with tab\n    - |");

            state.tab();
            assert_editor_eq(&state, "- parent item\n  - nested with tab\n|");
        }

        #[test]
        fn tab_with_trailing_newline() {
            // Cursor on line with newline after it - should still cycle correctly
            // Cycle: ["- ", "  ", "  - ", "    ", "    - ", ""]
            let mut state = editor_with_cursor("- parent item\n  - nested with tab\n|\n");

            state.tab();
            assert_editor_eq(&state, "- parent item\n  - nested with tab\n- |\n");

            state.tab();
            assert_editor_eq(&state, "- parent item\n  - nested with tab\n  |\n"); // para indent

            state.tab();
            assert_editor_eq(&state, "- parent item\n  - nested with tab\n  - |\n");

            state.tab();
            assert_editor_eq(&state, "- parent item\n  - nested with tab\n    |\n"); // nested para indent

            state.tab();
            assert_editor_eq(&state, "- parent item\n  - nested with tab\n    - |\n");

            state.tab();
            assert_editor_eq(&state, "- parent item\n  - nested with tab\n|\n");
        }

        #[test]
        fn tab_task_list_uses_list_marker_width_not_full_marker() {
            // Task list "- [ ] " is 6 chars, but para indent should use list marker width (2)
            // Cycle: ["- [ ] ", "  ", "  - [ ] ", ""]
            let mut state = editor_with_cursor("- [ ] hey\n\n|");

            state.tab();
            assert_editor_eq(&state, "- [ ] hey\n\n- [ ] |");

            state.tab();
            assert_editor_eq(&state, "- [ ] hey\n\n  |"); // 2 spaces, not 6

            state.tab();
            assert_editor_eq(&state, "- [ ] hey\n\n  - [ ] |"); // nested at 2 spaces

            state.tab();
            assert_editor_eq(&state, "- [ ] hey\n\n|");
        }
    }

    mod raw_cursor_movement_tests {
        use super::*;

        #[test]
        fn move_left_through_marker_is_atomic() {
            let mut state = editor_with_cursor("- |item");
            state.move_left();
            assert_editor_eq(&state, "|- item");
        }

        #[test]
        fn move_right_through_marker_is_atomic() {
            let mut state = editor_with_cursor("|- item");
            state.move_right();
            assert_editor_eq(&state, "- |item");
        }

        #[test]
        fn move_left_through_nested_markers_one_at_a_time() {
            let mut state = editor_with_cursor("> - |item");
            state.move_left();
            assert_editor_eq(&state, "> |- item");
            state.move_left();
            assert_editor_eq(&state, "|> - item");
        }

        #[test]
        fn move_left_does_not_skip_blank_lines() {
            let mut state = editor_with_cursor("line one\n\n|line three");
            state.move_left();
            assert_editor_eq(&state, "line one\n|\nline three");
        }

        #[test]
        fn move_left_from_blank_line_goes_to_previous() {
            let mut state = editor_with_cursor("line one\n|\nline three");
            state.move_left();
            assert_editor_eq(&state, "line one|\n\nline three");
        }

        #[test]
        fn move_up_maintains_column_in_content_area() {
            let mut state = editor_with_cursor("- item one\n- item |two");
            state.move_up();
            assert_editor_eq(&state, "- item |one\n- item two");
        }

        #[test]
        fn move_left_through_blockquote_ordered_list() {
            let mut state = editor_with_cursor("> 1. |");
            state.move_left();
            assert_editor_eq(&state, "> |1. ");
            state.move_left();
            assert_editor_eq(&state, "|> 1. ");
        }
    }

    mod checkbox_propagation_tests {
        use super::*;

        #[test]
        fn check_parent_checks_all_children() {
            let mut state = editor_with_cursor("- [ ] |parent\n  - [ ] child1\n  - [ ] child2\n");
            state.toggle_checkbox_state(0);
            let text = state.text();
            assert!(text.contains("[x] ~~parent~~"), "parent should be checked");
            assert!(text.contains("[x] ~~child1~~"), "child1 should be checked");
            assert!(text.contains("[x] ~~child2~~"), "child2 should be checked");
        }

        #[test]
        fn uncheck_parent_unchecks_all_children() {
            let mut state =
                editor_with_cursor("- [x] ~~|parent~~\n  - [x] ~~child1~~\n  - [x] ~~child2~~\n");
            state.toggle_checkbox_state(0);
            let text = state.text();
            assert!(text.contains("[ ] parent"), "parent should be unchecked");
            assert!(text.contains("[ ] child1"), "child1 should be unchecked");
            assert!(text.contains("[ ] child2"), "child2 should be unchecked");
            assert!(!text.contains("~~"), "no strikethrough should remain");
        }

        #[test]
        fn check_all_siblings_checks_parent() {
            let mut state =
                editor_with_cursor("- [ ] parent\n  - [x] ~~child1~~\n  - [ ] |child2\n");
            state.toggle_checkbox_state(2);
            let text = state.text();
            assert!(
                text.contains("[x] ~~parent~~"),
                "parent should be auto-checked"
            );
            assert!(
                text.contains("[x] ~~child1~~"),
                "child1 should remain checked"
            );
            assert!(text.contains("[x] ~~child2~~"), "child2 should be checked");
        }

        #[test]
        fn uncheck_child_unchecks_parent() {
            let mut state =
                editor_with_cursor("- [x] ~~parent~~\n  - [x] ~~|child1~~\n  - [x] ~~child2~~\n");
            state.toggle_checkbox_state(1);
            let text = state.text();
            assert!(text.contains("[ ] parent"), "parent should be unchecked");
            assert!(text.contains("[ ] child1"), "child1 should be unchecked");
            assert!(
                text.contains("[x] ~~child2~~"),
                "child2 should remain checked"
            );
        }

        #[test]
        fn deeply_nested_propagation_down() {
            let mut state = editor_with_cursor("- [ ] |level1\n  - [ ] level2\n    - [ ] level3\n");
            state.toggle_checkbox_state(0);
            let text = state.text();
            assert!(text.contains("[x] ~~level1~~"), "level1 should be checked");
            assert!(text.contains("[x] ~~level2~~"), "level2 should be checked");
            assert!(text.contains("[x] ~~level3~~"), "level3 should be checked");
        }

        #[test]
        fn deeply_nested_propagation_up() {
            let mut state = editor_with_cursor("- [ ] level1\n  - [ ] level2\n    - [ ] |level3\n");
            state.toggle_checkbox_state(2);
            let text = state.text();
            assert!(
                text.contains("[x] ~~level1~~"),
                "level1 should be auto-checked"
            );
            assert!(
                text.contains("[x] ~~level2~~"),
                "level2 should be auto-checked"
            );
            assert!(text.contains("[x] ~~level3~~"), "level3 should be checked");
        }

        #[test]
        fn mixed_siblings_parent_stays_unchecked() {
            let mut state = editor_with_cursor("- [ ] parent\n  - [ ] |child1\n  - [ ] child2\n");
            state.toggle_checkbox_state(1);
            let text = state.text();
            assert!(text.contains("[ ] parent"), "parent should stay unchecked");
            assert!(text.contains("[x] ~~child1~~"), "child1 should be checked");
            assert!(text.contains("[ ] child2"), "child2 should stay unchecked");
        }
    }
}

#[cfg(test)]
mod nested_context_tests {
    use super::*;

    #[test]
    fn nested_context_simple_list() {
        let state = EditorState::new("- item\n");
        let cursor_offset = 2; // on "item"
        let markers = state.build_nested_context(cursor_offset);
        assert_eq!(markers.len(), 1);
        assert!(matches!(
            markers[0],
            MarkerKind::ListItem { ordered: false, .. }
        ));
    }

    #[test]
    fn nested_context_nested_list() {
        let state = EditorState::new("- parent\n  - child\n");
        let cursor_offset = 14; // on "child"
        let markers = state.build_nested_context(cursor_offset);
        // Should show: - -
        assert_eq!(markers.len(), 2);
        assert!(matches!(
            markers[0],
            MarkerKind::ListItem { ordered: false, .. }
        ));
        assert!(matches!(
            markers[1],
            MarkerKind::ListItem { ordered: false, .. }
        ));
    }

    #[test]
    fn nested_context_checkbox_nested() {
        let state = EditorState::new("- [x] parent\n  - [ ] child\n");
        let cursor_offset = 20; // on "child"
        let markers = state.build_nested_context(cursor_offset);
        // Should show: - [x] - [ ]
        assert_eq!(markers.len(), 4);
        assert!(matches!(
            markers[0],
            MarkerKind::ListItem { ordered: false, .. }
        ));
        assert!(matches!(markers[1], MarkerKind::Checkbox { checked: true }));
        assert!(matches!(
            markers[2],
            MarkerKind::ListItem { ordered: false, .. }
        ));
        assert!(matches!(
            markers[3],
            MarkerKind::Checkbox { checked: false }
        ));
    }

    #[test]
    fn nested_context_blockquote_list() {
        let state = EditorState::new("> - item\n");
        let cursor_offset = 4; // on "item"
        let markers = state.build_nested_context(cursor_offset);
        // Should show: > -
        assert_eq!(markers.len(), 2);
        assert!(matches!(markers[0], MarkerKind::BlockQuote));
        assert!(matches!(
            markers[1],
            MarkerKind::ListItem { ordered: false, .. }
        ));
    }

    #[test]
    fn nested_context_ordered_list() {
        let state = EditorState::new("1. first\n2. second\n");
        let cursor_offset = 12; // on "second"
        let markers = state.build_nested_context(cursor_offset);
        assert_eq!(markers.len(), 1);
        assert!(matches!(
            markers[0],
            MarkerKind::ListItem { ordered: true, .. }
        ));
    }

    #[test]
    fn nested_context_empty_line() {
        let state = EditorState::new("hello\n");
        let cursor_offset = 2; // on "llo"
        let markers = state.build_nested_context(cursor_offset);
        assert_eq!(markers.len(), 0);
    }
}

#[cfg(test)]
mod debug_tree_structure {
    use super::*;

    #[test]
    fn check_blockquote_list_paragraph() {
        let state = EditorState::new("> - hey\n>   paragraph\n");

        if let Some(tree) = state.buffer.tree() {
            let root = tree.block_tree().root_node();
            log::debug!("Tree: {}", root.to_sexp());
        }
    }

    #[test]
    fn check_simple_list_paragraph() {
        let state = EditorState::new("- hey\n  paragraph\n");

        if let Some(tree) = state.buffer.tree() {
            let root = tree.block_tree().root_node();
            log::debug!("Tree: {}", root.to_sexp());
        }
    }
}

#[cfg(test)]
mod debug_tree_detail {
    use super::*;

    #[test]
    fn show_tree_detail() {
        let content = "> - hey\n>   paragraph\n";
        log::debug!("Content: {:?}", content);
        log::debug!("Bytes:");
        for (i, b) in content.bytes().enumerate() {
            log::debug!("  {}: {:?} ({})", i, b as char, b);
        }

        let state = EditorState::new(content);

        if let Some(tree) = state.buffer.tree() {
            let root = tree.block_tree().root_node();
            log::debug!("\nTree: {}", root.to_sexp());

            // Show each node with byte ranges
            fn print_node(node: tree_sitter::Node, indent: usize) {
                log::debug!(
                    "{}{} [{}-{}]",
                    "  ".repeat(indent),
                    node.kind(),
                    node.start_byte(),
                    node.end_byte()
                );
                for child in node.children(&mut node.walk()) {
                    print_node(child, indent + 1);
                }
            }
            print_node(root, 0);
        }
    }
}