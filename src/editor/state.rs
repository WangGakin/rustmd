
use tree_sitter;

use crate::buffer::Buffer;
use crate::cursor::{Cursor, Selection};
use crate::marker::{LineMarkers, MarkerKind, OrderedMarker, UnorderedMarker};

pub struct LineContext {
    /// Current cursor byte offset.
    pub cursor_offset: usize,
    /// Index of the current line.
    pub line_idx: usize,
    /// The current line's markers.
    pub line: LineMarkers,
    /// Whether content after markers is empty (whitespace only).
    pub is_empty: bool,
    /// Whether this line has any container markers.
    pub has_container: bool,
    /// The previous line, if any.
    pub prev_line: Option<LineMarkers>,
}

/// Cached tab cycle states for a specific line.
#[derive(Clone, Default)]
struct TabCycleCache {
    /// The line index this cache is for.
    line_idx: usize,
    /// The cached cycle states.
    states: Vec<String>,
}

/// The type of autocomplete trigger.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AutocompleteTrigger {
    /// User autocomplete triggered by `@`.
    User,
}

/// A suggestion from GitHub autocomplete.
#[derive(Clone)]
pub enum AutocompleteSuggestion {
    /// A GitHub user.
    User { login: String, name: Option<String> },
}

/// State for the autocomplete popup.
#[derive(Clone)]
pub struct AutocompleteState {
    /// The type of autocomplete (Issue or User).
    pub trigger: AutocompleteTrigger,
    /// Byte offset where the trigger character (`#` or `@`) was typed.
    pub trigger_offset: usize,
    /// The prefix typed after the trigger (e.g., "123" for `#123`).
    pub prefix: String,
    /// Suggestions fetched from GitHub.
    pub suggestions: Vec<AutocompleteSuggestion>,
    /// Currently selected suggestion index.
    pub selected_index: usize,
    /// Whether we're currently fetching suggestions.
    pub loading: bool,
    /// The prefix we last fetched for (to avoid duplicate fetches).
    pub fetched_prefix: Option<String>,
}

/// Core editing state that can be used without GPUI context.
/// This contains the buffer and selection, and all editing logic.
pub struct EditorState {
    pub buffer: Buffer,
    pub selection: Selection,
    /// Cached tab cycle states to avoid recalculating mid-cycle.
    tab_cycle_cache: Option<TabCycleCache>,
}

impl EditorState {
    pub fn new(content: &str) -> Self {
        let buffer: Buffer = content.parse().unwrap_or_default();
        Self {
            buffer,
            selection: Selection::new(0, 0),
            tab_cycle_cache: None,
        }
    }

    pub fn cursor(&self) -> Cursor {
        self.selection.cursor()
    }

    pub fn text(&self) -> String {
        self.buffer.text()
    }

    /// Set cursor position by byte offset.
    pub fn set_cursor(&mut self, offset: usize) {
        let offset = offset.min(self.buffer.len_bytes());
        self.selection = Selection::new(offset, offset);
    }

    /// Move cursor left by one character.
    pub fn move_left(&mut self) {
        let new_cursor = self.cursor().move_left(&self.buffer);
        self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
    }

    /// Move cursor right by one character.
    pub fn move_right(&mut self) {
        let new_cursor = self.cursor().move_right(&self.buffer);
        self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
    }

    /// Move cursor up by one line.
    pub fn move_up(&mut self) {
        let new_cursor = self.cursor().move_up(&self.buffer);
        self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
    }

    /// Move cursor down by one line.
    pub fn move_down(&mut self) {
        let new_cursor = self.cursor().move_down(&self.buffer);
        self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
    }

    /// Move cursor to start of current line.
    pub fn move_to_line_start(&mut self) {
        let new_cursor = self.cursor().move_to_line_start(&self.buffer);
        self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
    }

    /// Move cursor to end of current line.
    pub fn move_to_line_end(&mut self) {
        let new_cursor = self.cursor().move_to_line_end(&self.buffer);
        self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
    }

    /// Insert text at the current cursor position.
    pub fn insert_text(&mut self, text: &str) {
        // Clear tab cycle cache since content is changing
        self.tab_cycle_cache = None;
        let cursor_before = self.cursor().offset;
        let insert_pos = if !self.selection.is_collapsed() {
            let range = self.selection.range();
            self.buffer.delete(range.clone(), cursor_before);
            range.start
        } else {
            cursor_before
        };
        self.buffer.insert(insert_pos, text, insert_pos);
        let new_pos = insert_pos + text.len();
        self.selection = Selection::new(new_pos, new_pos);

        // After inserting, propagate checkbox state if this line has a checkbox.
        // This handles the case where tab cycling created an incomplete checkbox line
        // (e.g., "- [ ] ") and typing content makes it parseable by tree-sitter.
        self.propagate_checkbox_after_edit();
    }

    fn find_line_at(&self, byte_pos: usize) -> Option<(usize, LineMarkers)> {
        let idx = self.buffer.byte_to_line(byte_pos);
        if idx < self.buffer.line_count() {
            Some((idx, self.buffer.line_markers(idx)))
        } else {
            None
        }
    }

    /// Check if the cursor is inside a code block (between opening and closing fences,
    /// or after an opening fence with no closing fence yet).
    pub(crate) fn cursor_in_code_block(&self) -> bool {
        let Some(tree) = self.buffer.tree() else {
            return false;
        };

        let cursor_offset = self.cursor().offset;
        let root = tree.block_tree().root_node();

        // Find the deepest node at the cursor position and walk up looking for fenced_code_block
        let Some(node) = root.descendant_for_byte_range(cursor_offset, cursor_offset) else {
            return false;
        };

        let mut current = Some(node);
        while let Some(n) = current {
            if n.kind() == "fenced_code_block" {
                return true;
            }
            current = n.parent();
        }
        false
    }

    /// Check if a line has content after its markers.
    /// Lines with code fences are always considered to have content.
    fn line_has_content(&self, line: &LineMarkers) -> bool {
        if line.is_fence() {
            return true;
        }
        let content_start = line
            .marker_range()
            .map(|r| r.end)
            .unwrap_or(line.range.start);
        !self
            .buffer
            .slice_cow(content_start..line.range.end)
            .trim()
            .is_empty()
    }

    /// Get context about the line at the cursor.
    /// Returns None if the cursor is not on a valid line.
    fn line_context(&self) -> Option<LineContext> {
        let cursor_offset = self.cursor().offset;
        let line_idx = self.buffer.byte_to_line(cursor_offset);
        if line_idx >= self.buffer.line_count() {
            return None;
        }
        let line = self.buffer.line_markers(line_idx);

        let is_empty = !self.line_has_content(&line);
        let has_container = line.has_container();

        let prev_line = if line_idx > 0 {
            Some(self.buffer.line_markers(line_idx - 1))
        } else {
            None
        };

        Some(LineContext {
            cursor_offset,
            line_idx,
            line,
            is_empty,
            has_container,
            prev_line,
        })
    }

    /// Auto-insert space after `>` if it just became a blockquote marker.
    /// Returns true if a space was inserted.
    pub fn maybe_complete_blockquote_marker(&mut self) -> bool {
        let cursor_pos = self.cursor().offset;
        if cursor_pos == 0 {
            return false;
        }

        if self.buffer.byte_at(cursor_pos - 1) != Some(b'>') {
            return false;
        }

        if self.buffer.byte_at(cursor_pos) == Some(b' ') {
            return false;
        }

        let line_idx = self.buffer.byte_to_line(cursor_pos);
        if line_idx >= self.buffer.line_count() {
            return false;
        }
        let line = self.buffer.line_markers(line_idx);

        let has_blockquote = line
            .markers
            .iter()
            .any(|m| matches!(m.kind, MarkerKind::BlockQuote));

        if !has_blockquote {
            return false;
        }

        self.insert_text(" ");
        true
    }

    /// After typing ` or ~, check if we just completed "```" or "~~~" at line start
    /// and auto-insert the closing fence.
    pub fn maybe_complete_code_fence(&mut self) {
        let cursor_pos = self.cursor().offset;
        if cursor_pos < 3 {
            return;
        }

        // Check we just typed 3 of the same fence character
        let fence_char = self.buffer.byte_at(cursor_pos - 1);
        if fence_char != Some(b'`') && fence_char != Some(b'~') {
            return;
        }
        if self.buffer.byte_at(cursor_pos - 2) != fence_char
            || self.buffer.byte_at(cursor_pos - 3) != fence_char
        {
            return;
        }

        // Check this is at the start of a line (possibly after blockquote markers)
        let line_idx = self.buffer.byte_to_line(cursor_pos);
        let line_start = self.buffer.line_to_byte(line_idx);
        let before_fence = self.buffer.slice_cow(line_start..(cursor_pos - 3));
        let trimmed = before_fence.trim();

        // Allow only whitespace or blockquote markers before the fence
        if !trimmed.is_empty() && !trimmed.chars().all(|c| c == '>') {
            return;
        }

        // Insert newline + closing fence, cursor stays after opening fence
        let closing = if fence_char == Some(b'`') {
            "\n```"
        } else {
            "\n~~~"
        };
        self.buffer.insert(cursor_pos, closing, cursor_pos);
    }

    /// Try to insert a space. Returns false if space should be ignored
    /// (at line start, or at blockquote content start outside code blocks).
    pub fn try_insert_space(&mut self) -> bool {
        if self.cursor_in_code_block() {
            self.insert_text(" ");
            return true;
        }

        let cursor = self.cursor();
        let line_start = cursor.move_to_line_start(&self.buffer).offset;

        if cursor.offset == line_start || self.cursor_at_blockquote_content_start() {
            self.insert_text(" ");
            return true;
        }

        self.insert_text(" ");
        true
    }

    /// Check if cursor is at the content start of a blockquote-only line.
    /// Used to prevent inserting spaces/tabs at the "beginning" of blockquote content.
    fn cursor_at_blockquote_content_start(&self) -> bool {
        let cursor_pos = self.cursor().offset;
        let line_idx = self.buffer.byte_to_line(cursor_pos);
        if line_idx >= self.buffer.line_count() {
            return false;
        }
        let line = self.buffer.line_markers(line_idx);

        if !line.is_blockquote_only() {
            return false;
        }

        if let Some(marker_range) = line.marker_range() {
            cursor_pos == marker_range.end
        } else {
            false
        }
    }

    /// Tab: cycle forward through nesting states based on tree-sitter context.
    pub fn tab(&mut self) {
        let Some((states, current_idx, prefix_end)) = self.get_tab_cycle_state() else {
            return;
        };

        if states.len() <= 1 {
            return;
        }

        let next_idx = (current_idx + 1) % states.len();
        self.set_line_prefix(&states[next_idx], prefix_end);

        // After changing structure, propagate checkbox state if this line has a checkbox
        self.propagate_checkbox_after_edit();
    }

    /// Shift+Tab: cycle backward through nesting states.
    fn shift_tab_cycle(&mut self) {
        let Some((states, current_idx, prefix_end)) = self.get_tab_cycle_state() else {
            return;
        };

        if states.len() <= 1 {
            return;
        }

        let prev_idx = if current_idx == 0 {
            states.len() - 1
        } else {
            current_idx - 1
        };
        self.set_line_prefix(&states[prev_idx], prefix_end);

        // After changing structure, propagate checkbox state if this line has a checkbox
        self.propagate_checkbox_after_edit();
    }

    /// Get tab cycle states, using cache if available for current line.
    /// Returns (states, current_idx, prefix_end) where prefix_end is where the prefix ends.
    fn get_tab_cycle_state(&mut self) -> Option<(Vec<String>, usize, usize)> {
        let cursor_offset = self.cursor().offset;
        let line_idx = self.buffer.byte_to_line(cursor_offset);
        let line_start = self.buffer.line_to_byte(line_idx);

        // Get current line's checkbox state to pass to state builder
        let current_checkbox = self.buffer.line_markers(line_idx).checkbox();

        // Check if we have a valid cache for this line
        let states = if let Some(ref cache) = self.tab_cycle_cache {
            if cache.line_idx == line_idx {
                cache.states.clone()
            } else {
                // Different line, recalculate and cache
                let states = self.build_cycle_states_from_tree(cursor_offset, current_checkbox);
                self.tab_cycle_cache = Some(TabCycleCache {
                    line_idx,
                    states: states.clone(),
                });
                states
            }
        } else {
            // No cache, calculate and cache
            let states = self.build_cycle_states_from_tree(cursor_offset, current_checkbox);
            self.tab_cycle_cache = Some(TabCycleCache {
                line_idx,
                states: states.clone(),
            });
            states
        };

        if states.len() <= 1 {
            return None;
        }

        // Find which state matches the current line's prefix
        // We check if the line starts with each state (longest match wins)
        let line_end = self
            .buffer
            .line_to_byte(line_idx + 1)
            .min(self.buffer.len_bytes());
        let line_text = self.buffer.slice_cow(line_start..line_end);

        let mut best_match: Option<(usize, &str)> = None;
        for (idx, state) in states.iter().enumerate() {
            if line_text.starts_with(state)
                && (best_match.is_none() || state.len() > best_match.unwrap().1.len())
            {
                best_match = Some((idx, state));
            }
        }

        let (current_idx, prefix_end) = match best_match {
            Some((idx, state)) => (idx, line_start + state.len()),
            None => (0, line_start), // Default to empty prefix at index 0
        };

        Some((states, current_idx, prefix_end))
    }

    /// Build tab cycle states by walking up the tree-sitter parse tree.
    /// The cycle is determined by context ABOVE the current line, not by current line content.
    /// If `checkbox_state` is Some, task list markers will use that state instead of the parent's.
    pub fn build_cycle_states_from_tree(
        &self,
        cursor_offset: usize,
        checkbox_state: Option<bool>,
    ) -> Vec<String> {
        let Some(tree) = self.buffer.tree() else {
            return vec![String::new()];
        };

        let root = tree.block_tree().root_node();
        let cursor_line_idx = self.buffer.byte_to_line(cursor_offset);

        let line_start = self.buffer.line_to_byte(cursor_line_idx);
        let lookup_offset = if line_start > 0 { line_start - 1 } else { 0 };
        let node = root.descendant_for_byte_range(lookup_offset, lookup_offset);

        let Some(node) = node else {
            return vec![String::new()];
        };

        let context_node = if self.is_in_error_node(node) {
            self.find_context_from_error(node).unwrap_or(node)
        } else {
            node
        };

        let mut nodes_to_process: Vec<tree_sitter::Node> = Vec::new();
        let mut blockquote_prefix = String::new();
        let mut current = Some(context_node);

        while let Some(n) = current {
            if n.kind() == "block_quote" {
                if let Some(marker_node) = n
                    .children(&mut n.walk())
                    .find(|c| c.kind() == "block_quote_marker")
                {
                    let marker_text = self
                        .buffer
                        .slice_cow(marker_node.start_byte()..marker_node.end_byte());
                    blockquote_prefix = format!("{}{}", marker_text, blockquote_prefix);
                }
            } else if n.kind() == "list_item" {
                nodes_to_process.push(n);
            }
            current = n.parent();
        }

        let mut list_levels: Vec<(usize, String, usize, bool)> = Vec::new();

        for n in nodes_to_process {
            let mut marker_text = String::new();
            let mut list_marker_len = 0;
            let mut marker_start = 0;
            let mut is_ordered = false;

            for child in n.children(&mut n.walk()) {
                match child.kind() {
                    "list_marker_minus" | "list_marker_plus" | "list_marker_star" => {
                        marker_start = child.start_byte();
                        let text = self.buffer.slice_cow(child.start_byte()..child.end_byte());
                        list_marker_len = text.len();
                        marker_text.push_str(&text);
                    }
                    "list_marker_dot" | "list_marker_parenthesis" => {
                        marker_start = child.start_byte();
                        let text = self.buffer.slice_cow(child.start_byte()..child.end_byte());
                        list_marker_len = text.len();
                        marker_text.push_str(&text);
                        is_ordered = true;
                    }
                    "task_list_marker_checked" | "task_list_marker_unchecked" => {
                        // Use the current line's checkbox state if provided.
                        // If None (line has no checkbox yet), default to unchecked.
                        let checkbox_text = match checkbox_state {
                            Some(true) => "[x]",
                            Some(false) | None => "[ ]",
                        };
                        marker_text.push_str(checkbox_text);
                        marker_text.push(' ');
                    }
                    _ => {}
                }
            }

            if !marker_text.is_empty() {
                let line_idx = self.buffer.byte_to_line(marker_start);
                let line_start = self.buffer.line_to_byte(line_idx);
                let absolute_indent = marker_start - line_start;
                let indent = absolute_indent.saturating_sub(blockquote_prefix.len());
                list_levels.push((indent, marker_text, list_marker_len, is_ordered));
            }
        }

        if list_levels.is_empty() && blockquote_prefix.is_empty() {
            return vec![String::new()];
        }

        list_levels.reverse();

        let mut states = Vec::new();

        if !blockquote_prefix.is_empty() {
            states.push(blockquote_prefix.clone());
        }

        for (indent, marker, list_marker_len, is_ordered) in &list_levels {
            let sibling_marker = if *is_ordered {
                Self::increment_ordered_marker(marker)
            } else {
                marker.clone()
            };
            states.push(format!(
                "{}{}{}",
                blockquote_prefix,
                " ".repeat(*indent),
                sibling_marker
            ));

            states.push(format!(
                "{}{}",
                blockquote_prefix,
                " ".repeat(indent + list_marker_len)
            ));
        }

        if let Some((deepest_indent, deepest_marker, list_marker_len, is_ordered)) =
            list_levels.last()
        {
            let deeper_indent = deepest_indent + list_marker_len;
            let nested_marker = if *is_ordered {
                Self::reset_ordered_marker(deepest_marker)
            } else {
                deepest_marker.clone()
            };
            states.push(format!(
                "{}{}{}",
                blockquote_prefix,
                " ".repeat(deeper_indent),
                nested_marker
            ));
        }

        states.push(String::new());
        states
    }

    fn increment_ordered_marker(marker: &str) -> String {
        let num_end = marker
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(marker.len());
        if num_end == 0 {
            return marker.to_string();
        }
        let num: usize = marker[..num_end].parse().unwrap_or(1);
        format!("{}{}", num + 1, &marker[num_end..])
    }

    fn reset_ordered_marker(marker: &str) -> String {
        let num_end = marker
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(marker.len());
        if num_end == 0 {
            return marker.to_string();
        }
        format!("1{}", &marker[num_end..])
    }

    fn is_in_error_node(&self, node: tree_sitter::Node) -> bool {
        let mut current = Some(node);
        while let Some(n) = current {
            if n.kind() == "ERROR" {
                return true;
            }
            current = n.parent();
        }
        false
    }

    fn find_context_from_error<'a>(
        &self,
        node: tree_sitter::Node<'a>,
    ) -> Option<tree_sitter::Node<'a>> {
        let mut current = Some(node);
        while let Some(n) = current {
            if n.kind() == "ERROR" {
                if let Some(prev) = n.prev_sibling() {
                    return self.find_last_list_item(prev);
                }
                return None;
            }
            current = n.parent();
        }
        None
    }

    fn find_last_list_item<'a>(
        &self,
        node: tree_sitter::Node<'a>,
    ) -> Option<tree_sitter::Node<'a>> {
        let mut result: Option<tree_sitter::Node<'a>> = None;
        if node.kind() == "list_item" {
            result = Some(node);
        }
        let child_count = node.child_count();
        for i in (0..child_count).rev() {
            if let Some(child) = node.child(i as u32)
                && let Some(found) = self.find_last_list_item(child)
            {
                return Some(found);
            }
        }
        result
    }

    /// Find the list_item node containing the given byte offset.
    fn find_list_item_node(&self, byte_offset: usize) -> Option<tree_sitter::Node<'_>> {
        let tree = self.buffer.tree()?;
        let root = tree.block_tree().root_node();
        let node = root.descendant_for_byte_range(byte_offset, byte_offset)?;

        let mut current = Some(node);
        while let Some(n) = current {
            if n.kind() == "list_item" {
                return Some(n);
            }
            current = n.parent();
        }
        None
    }

    /// Find all checkboxes nested within a list_item node.
    /// Returns Vec of (checkbox_byte_offset, is_checked).
    fn find_nested_checkboxes(&self, list_item_node: tree_sitter::Node) -> Vec<(usize, bool)> {
        let mut checkboxes = Vec::new();
        let mut cursor = list_item_node.walk();

        loop {
            let node = cursor.node();
            match node.kind() {
                "task_list_marker_checked" => {
                    checkboxes.push((node.start_byte(), true));
                }
                "task_list_marker_unchecked" => {
                    checkboxes.push((node.start_byte(), false));
                }
                _ => {}
            }

            if cursor.goto_first_child() {
                continue;
            }
            if cursor.goto_next_sibling() {
                continue;
            }
            loop {
                if !cursor.goto_parent() {
                    return checkboxes;
                }
                if cursor.node().id() == list_item_node.id() {
                    return checkboxes;
                }
                if cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    /// Build full nested context markers by walking up the tree-sitter tree.
    /// Returns markers from outermost to innermost (e.g., `> - [x] - [ ]`).
    pub fn build_nested_context(&self, cursor_offset: usize) -> Vec<MarkerKind> {
        let Some(tree) = self.buffer.tree() else {
            return Vec::new();
        };

        let root = tree.block_tree().root_node();

        // Handle edge case: cursor at end of file
        let lookup_offset = if cursor_offset > 0
            && root
                .descendant_for_byte_range(cursor_offset, cursor_offset)
                .map(|n| n.kind() == "document")
                .unwrap_or(true)
        {
            cursor_offset - 1
        } else {
            cursor_offset
        };

        let Some(node) = root.descendant_for_byte_range(lookup_offset, lookup_offset) else {
            return Vec::new();
        };

        // Walk up from current node, collecting context from each relevant ancestor
        let mut markers_reversed = Vec::new();
        let mut current = Some(node);

        while let Some(n) = current {
            match n.kind() {
                "block_quote" => {
                    markers_reversed.push(MarkerKind::BlockQuote);
                }
                "list_item" => {
                    // Scan direct children for list marker and checkbox
                    // Collect in reverse order (checkbox then list_marker) because
                    // we reverse the whole list at the end, so we want: - [x]
                    let mut list_marker: Option<MarkerKind> = None;
                    let mut checkbox: Option<MarkerKind> = None;

                    let mut cursor = n.walk();
                    if cursor.goto_first_child() {
                        loop {
                            let child = cursor.node();
                            match child.kind() {
                                "task_list_marker_checked" => {
                                    checkbox = Some(MarkerKind::Checkbox { checked: true });
                                }
                                "task_list_marker_unchecked" => {
                                    checkbox = Some(MarkerKind::Checkbox { checked: false });
                                }
                                "list_marker_minus" => {
                                    list_marker = Some(MarkerKind::ListItem {
                                        ordered: false,
                                        unordered_marker: Some(UnorderedMarker::Minus),
                                        ordered_marker: None,
                                        number: None,
                                    });
                                }
                                "list_marker_star" => {
                                    list_marker = Some(MarkerKind::ListItem {
                                        ordered: false,
                                        unordered_marker: Some(UnorderedMarker::Star),
                                        ordered_marker: None,
                                        number: None,
                                    });
                                }
                                "list_marker_plus" => {
                                    list_marker = Some(MarkerKind::ListItem {
                                        ordered: false,
                                        unordered_marker: Some(UnorderedMarker::Plus),
                                        ordered_marker: None,
                                        number: None,
                                    });
                                }
                                "list_marker_dot" | "list_marker_parenthesis" => {
                                    // Extract the number from the marker text
                                    let marker_text =
                                        self.buffer.slice_cow(child.start_byte()..child.end_byte());
                                    let number = marker_text
                                        .trim()
                                        .chars()
                                        .take_while(|c| c.is_ascii_digit())
                                        .collect::<String>()
                                        .parse::<u32>()
                                        .ok();
                                    let ordered_marker =
                                        Some(if child.kind() == "list_marker_dot" {
                                            OrderedMarker::Dot
                                        } else {
                                            OrderedMarker::Parenthesis
                                        });
                                    list_marker = Some(MarkerKind::ListItem {
                                        ordered: true,
                                        unordered_marker: None,
                                        ordered_marker,
                                        number,
                                    });
                                }
                                _ => {}
                            }
                            if !cursor.goto_next_sibling() {
                                break;
                            }
                        }
                    }

                    // Add in reverse order: checkbox first, then list_marker
                    // After final reverse, this becomes: list_marker, checkbox (i.e., "- [x]")
                    if let Some(cb) = checkbox {
                        markers_reversed.push(cb);
                    }
                    if let Some(lm) = list_marker {
                        markers_reversed.push(lm);
                    }
                }
                "fenced_code_block" => {
                    // Find info_string for language
                    let mut cursor = n.walk();
                    let mut language = None;
                    if cursor.goto_first_child() {
                        loop {
                            let child = cursor.node();
                            if child.kind() == "info_string" {
                                language = Some(
                                    self.buffer
                                        .slice_cow(child.start_byte()..child.end_byte())
                                        .to_string(),
                                );
                                break;
                            }
                            if !cursor.goto_next_sibling() {
                                break;
                            }
                        }
                    }
                    markers_reversed.push(MarkerKind::CodeBlockFence {
                        language,
                        is_opening: true,
                    });
                }
                _ => {}
            }
            current = n.parent();
        }

        // Reverse to get outermost-to-innermost order
        markers_reversed.reverse();
        markers_reversed
    }

    /// Find the parent list_item's checkbox, if any.
    /// Returns (checkbox_byte_offset, is_checked).
    fn find_parent_checkbox(&self, list_item_start: usize) -> Option<(usize, bool)> {
        let tree = self.buffer.tree()?;
        let root = tree.block_tree().root_node();
        let node = root.descendant_for_byte_range(list_item_start, list_item_start)?;

        // Find our list_item first
        let mut current = Some(node);
        let mut our_list_item = None;
        while let Some(n) = current {
            if n.kind() == "list_item" {
                our_list_item = Some(n);
                break;
            }
            current = n.parent();
        }

        // Walk up to find parent list_item
        let our_list_item = our_list_item?;
        let mut current = our_list_item.parent();
        while let Some(n) = current {
            if n.kind() == "list_item" {
                // Found parent list_item, find its checkbox among direct children
                let mut cursor = n.walk();
                if cursor.goto_first_child() {
                    loop {
                        let child = cursor.node();
                        match child.kind() {
                            "task_list_marker_checked" => {
                                return Some((child.start_byte(), true));
                            }
                            "task_list_marker_unchecked" => {
                                return Some((child.start_byte(), false));
                            }
                            _ => {}
                        }
                        if !cursor.goto_next_sibling() {
                            break;
                        }
                    }
                }
                return None;
            }
            current = n.parent();
        }
        None
    }

    /// Find all sibling checkboxes (same nesting level).
    /// Returns Vec of (checkbox_byte_offset, is_checked).
    fn find_sibling_checkboxes(&self, list_item_start: usize) -> Vec<(usize, bool)> {
        let tree = match self.buffer.tree() {
            Some(t) => t,
            None => return Vec::new(),
        };
        let root = tree.block_tree().root_node();
        let node = match root.descendant_for_byte_range(list_item_start, list_item_start) {
            Some(n) => n,
            None => return Vec::new(),
        };

        // Find our list_item
        let mut current = Some(node);
        let mut our_list_item = None;
        while let Some(n) = current {
            if n.kind() == "list_item" {
                our_list_item = Some(n);
                break;
            }
            current = n.parent();
        }

        let our_list_item = match our_list_item {
            Some(n) => n,
            None => return Vec::new(),
        };

        // Get parent list node
        let parent_list = match our_list_item.parent() {
            Some(p) if p.kind() == "list" => p,
            _ => return Vec::new(),
        };

        // Iterate all list_item children and collect their checkboxes
        let mut siblings = Vec::new();
        let mut cursor = parent_list.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.kind() == "list_item" {
                    // Find checkbox in this list_item (direct child only)
                    let mut inner_cursor = child.walk();
                    if inner_cursor.goto_first_child() {
                        loop {
                            let inner_child = inner_cursor.node();
                            match inner_child.kind() {
                                "task_list_marker_checked" => {
                                    siblings.push((inner_child.start_byte(), true));
                                    break;
                                }
                                "task_list_marker_unchecked" => {
                                    siblings.push((inner_child.start_byte(), false));
                                    break;
                                }
                                _ => {}
                            }
                            if !inner_cursor.goto_next_sibling() {
                                break;
                            }
                        }
                    }
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        siblings
    }

    /// Set the line prefix, replacing current markers up to prefix_end.
    /// Preserves any content after prefix_end and adjusts cursor position.
    fn set_line_prefix(&mut self, new_prefix: &str, prefix_end: usize) {
        let cursor_offset = self.cursor().offset;
        let line_idx = self.buffer.byte_to_line(cursor_offset);
        let line_start = self.buffer.line_to_byte(line_idx);

        let old_prefix_len = prefix_end - line_start;
        let new_prefix_len = new_prefix.len();
        let len_diff = new_prefix_len as isize - old_prefix_len as isize;

        // Delete old prefix
        if prefix_end > line_start {
            self.buffer.delete(line_start..prefix_end, cursor_offset);
        }

        // Insert new prefix
        if !new_prefix.is_empty() {
            self.buffer.insert(line_start, new_prefix, line_start);
        }

        // Adjust cursor: if cursor was after prefix, shift by the length difference
        // If cursor was in the prefix area, move to end of new prefix
        let new_cursor = if cursor_offset >= prefix_end {
            (cursor_offset as isize + len_diff) as usize
        } else {
            line_start + new_prefix_len
        };
        self.selection = Selection::new(new_cursor, new_cursor);
    }

    /// Smart enter: creates paragraph break or exits container on empty line.
    /// Enter: just insert a raw newline. No magic.
    pub fn enter(&mut self) {
        self.insert_text("\n");
    }

    /// Shift+Enter: continue container (add markers from current line).
    /// In code blocks, copies leading whitespace for indentation.
    pub fn shift_enter(&mut self) {
        // In code blocks, copy leading whitespace from current line
        if self.cursor_in_code_block() {
            let indent = self.current_line_leading_whitespace();
            self.insert_text("\n");
            if !indent.is_empty() {
                self.insert_text(&indent);
            }
            return;
        }

        let Some(ctx) = self.line_context() else {
            self.insert_text("\n");
            return;
        };

        let continuation = ctx.line.continuation_rope(self.buffer.rope());
        self.insert_text("\n");
        if !continuation.is_empty() {
            self.insert_text(&continuation);
        }
    }

    /// Get leading whitespace (spaces/tabs) from the current line.
    fn current_line_leading_whitespace(&self) -> String {
        let cursor = self.cursor();
        let line_start = cursor.move_to_line_start(&self.buffer).offset;
        let line_end = cursor.move_to_line_end(&self.buffer).offset;
        let line_text = self.buffer.slice_cow(line_start..line_end);

        line_text
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    }

    /// Shift+Alt+Enter: create indented continuation (for nested paragraphs).
    /// For lists: newline + indent (no list marker)
    /// For blockquotes alone: newline + indent (exits blockquote)
    /// For nested (e.g. `> - item`): newline + outer markers + indent
    pub fn shift_alt_enter(&mut self) {
        let indent = {
            let Some(ctx) = self.line_context() else {
                self.insert_text("\n");
                return;
            };

            let has_list = ctx
                .line
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::ListItem { .. }));
            let has_blockquote = ctx
                .line
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::BlockQuote));

            if has_blockquote && !has_list {
                "  ".to_string()
            } else {
                ctx.line.nested_paragraph_indent(self.buffer.rope())
            }
        };

        self.insert_text("\n");
        if !indent.is_empty() {
            self.insert_text(&indent);
        }
    }

    /// Shift+Tab: cycle backward through nesting states.
    pub fn shift_tab(&mut self) {
        self.shift_tab_cycle();
    }

    fn backspace_range_with_type(
        &self,
        cursor_pos: usize,
    ) -> Option<(std::ops::Range<usize>, bool)> {
        let (_, line) = self.find_line_at(cursor_pos)?;

        for marker in &line.markers {
            if cursor_pos == marker.range.end {
                let is_indent = matches!(marker.kind, MarkerKind::Indent);
                return Some((marker.range.clone(), is_indent));
            }
        }

        None
    }

    /// If cursor is at end of an opening code fence and the code block contains
    /// only whitespace, return the full block range to delete.
    fn find_empty_code_block_range(&self, cursor_pos: usize) -> Option<std::ops::Range<usize>> {
        let tree = self.buffer.tree()?;
        let root = tree.block_tree().root_node();

        // Find the node at cursor position (look slightly before since cursor is at end of fence)
        let node = root.descendant_for_byte_range(cursor_pos.saturating_sub(1), cursor_pos)?;

        // Walk up to find fenced_code_block
        let mut current = Some(node);
        let code_block = loop {
            match current {
                Some(n) if n.kind() == "fenced_code_block" => break n,
                Some(n) => current = n.parent(),
                None => return None,
            }
        };

        let block_start = code_block.start_byte();
        let block_end = code_block.end_byte();

        // Find where content starts (after first line / opening fence)
        let block_text = self.buffer.slice_cow(block_start..block_end);
        let first_newline = block_text.find('\n')?;
        let content_start = block_start + first_newline + 1;

        // Check if content (between opening fence and end) is only whitespace + closing fence
        let content = self.buffer.slice_cow(content_start..block_end);
        let trimmed = content.trim();

        if trimmed == "```" || trimmed == "~~~" {
            // Don't include trailing newline after closing fence
            let mut end = block_end;
            if self.buffer.byte_at(end.saturating_sub(1)) == Some(b'\n') {
                end -= 1;
            }
            Some(block_start..end)
        } else {
            None
        }
    }

    /// Delete backward (backspace). Simple: delete one unit.
    /// Markers and indents are atomic - deleted as a whole.
    pub fn delete_backward(&mut self) {
        // Clear tab cycle cache since content is changing
        self.tab_cycle_cache = None;
        if !self.selection.is_collapsed() {
            self.delete_selection();
            self.propagate_checkbox_after_edit();
            return;
        }

        if self.cursor().offset == 0 {
            return;
        }

        let cursor_pos = self.cursor().offset;

        if let Some((marker_range, _is_indent)) = self.backspace_range_with_type(cursor_pos) {
            // Check if we're deleting an opening code fence of an empty code block
            if let Some(block_range) = self.find_empty_code_block_range(cursor_pos) {
                // Delete the entire empty code block
                self.buffer.delete(block_range.clone(), cursor_pos);
                self.selection = Selection::new(block_range.start, block_range.start);
                self.propagate_checkbox_after_edit();
                return;
            }

            // Otherwise just delete the marker
            self.buffer.delete(marker_range.clone(), cursor_pos);
            self.selection = Selection::new(marker_range.start, marker_range.start);
            self.propagate_checkbox_after_edit();
            return;
        }

        let new_pos = self.buffer.prev_char_boundary(cursor_pos);
        self.buffer.delete(new_pos..cursor_pos, cursor_pos);
        self.selection = Selection::new(new_pos, new_pos);
        self.propagate_checkbox_after_edit();
    }

    pub(crate) fn delete_selection(&mut self) {
        let range = self.selection.range();
        let cursor_before = self.cursor().offset;
        self.buffer.delete(range.clone(), cursor_before);
        self.selection = Selection::new(range.start, range.start);
    }

    /// Delete the character after the cursor, or the selection if active.
    pub fn delete_forward(&mut self) {
        // Clear tab cycle cache since content is changing
        self.tab_cycle_cache = None;
        if !self.selection.is_collapsed() {
            self.delete_selection();
        } else if self.cursor().offset < self.buffer.len_bytes() {
            let cursor_before = self.cursor().offset;
            let next = self.cursor().move_right(&self.buffer);
            self.buffer
                .delete(cursor_before..next.offset, cursor_before);
        }
        self.propagate_checkbox_after_edit();
    }

    pub fn handle_click(&mut self, buffer_offset: usize, shift_held: bool, click_count: usize) {
        if shift_held {
            self.selection = self.selection.extend_to(buffer_offset);
        } else {
            match click_count {
                2 => {
                    self.selection = Selection::select_word_at(buffer_offset, &self.buffer);
                }
                3 => {
                    self.selection = Selection::select_line_at(buffer_offset, &self.buffer);
                }
                _ => {
                    self.selection = Selection::new(buffer_offset, buffer_offset);
                }
            }
        }
    }

    pub fn handle_drag(&mut self, buffer_offset: usize) {
        self.selection = self.selection.extend_to(buffer_offset);
    }

    /// Toggle a checkbox on the given line, propagating to children and parents.
    pub fn toggle_checkbox_state(&mut self, line_number: usize) {
        let (is_checked, checkbox_byte_start) = {
            if line_number >= self.buffer.line_count() {
                return;
            }
            let line = self.buffer.line_markers(line_number);

            let Some(is_checked) = line.checkbox() else {
                return;
            };

            let line_text = self.buffer.slice_cow(line.range.clone());
            let checkbox_pattern = if is_checked { "[x]" } else { "[ ]" };
            let alt_pattern = if is_checked { "[X]" } else { "" };

            let checkbox_offset = line_text.find(checkbox_pattern).or_else(|| {
                if !alt_pattern.is_empty() {
                    line_text.find(alt_pattern)
                } else {
                    None
                }
            });

            let Some(relative_offset) = checkbox_offset else {
                return;
            };

            let checkbox_byte_start = line.range.start + relative_offset;
            (is_checked, checkbox_byte_start)
        };

        let new_checked = !is_checked;
        let mut cursor_pos = self.cursor().offset;

        // Find the list_item node for this checkbox - use checkbox_byte_start for accurate node finding
        let list_item_node = self.find_list_item_node(checkbox_byte_start);

        // Collect all checkboxes to toggle (clicked + nested children)
        let mut checkboxes_to_toggle: Vec<(usize, bool)> = Vec::new();

        if let Some(node) = list_item_node {
            // Get all nested checkboxes within this list_item
            let nested = self.find_nested_checkboxes(node);
            for (offset, currently_checked) in nested {
                // Only toggle if state differs from target
                if currently_checked != new_checked {
                    checkboxes_to_toggle.push((offset, currently_checked));
                }
            }
        } else {
            // No list_item found, just toggle the clicked checkbox
            checkboxes_to_toggle.push((checkbox_byte_start, is_checked));
        }

        // Sort by offset descending so we can modify without invalidating earlier offsets
        checkboxes_to_toggle.sort_unstable_by_key(|a| std::cmp::Reverse(a.0));

        // Toggle each checkbox
        for (offset, _currently_checked) in &checkboxes_to_toggle {
            let content_start = offset + 1; // skip '['
            let content_end = content_start + 1;
            let new_content = if new_checked { "x" } else { " " };
            self.buffer
                .replace(content_start..content_end, new_content, cursor_pos);
        }

        // Handle strikethrough for each toggled checkbox's line
        // Process in reverse order (highest offset first) since strikethrough changes byte offsets
        for (offset, _) in &checkboxes_to_toggle {
            let line_idx = self.buffer.byte_to_line(*offset);
            let adjustment = self.toggle_line_strikethrough(line_idx, new_checked, cursor_pos);
            cursor_pos = (cursor_pos as isize + adjustment) as usize;
        }

        // Propagate upward: if checking and all siblings are now checked, check parent
        // If unchecking, uncheck parent if it was checked
        self.propagate_checkbox_up(checkbox_byte_start, new_checked, &mut cursor_pos);

        self.selection = Selection::new(cursor_pos, cursor_pos);
    }

    /// Propagate checkbox state upward through parent list items.
    fn propagate_checkbox_up(
        &mut self,
        list_item_start: usize,
        checked: bool,
        cursor_pos: &mut usize,
    ) {
        // Find parent checkbox
        let parent_info = self.find_parent_checkbox(list_item_start);
        let Some((parent_offset, parent_checked)) = parent_info else {
            return;
        };

        if checked {
            // When checking: only auto-check parent if ALL siblings are now checked
            let siblings = self.find_sibling_checkboxes(list_item_start);
            let all_checked = siblings.iter().all(|(_, is_checked)| *is_checked);

            if all_checked && !parent_checked {
                // Check the parent
                let content_start = parent_offset + 1;
                let content_end = content_start + 1;
                self.buffer
                    .replace(content_start..content_end, "x", *cursor_pos);

                // Toggle strikethrough for parent's direct content line
                let parent_line = self.buffer.byte_to_line(parent_offset);
                let adjustment = self.toggle_line_strikethrough(parent_line, true, *cursor_pos);
                *cursor_pos = (*cursor_pos as isize + adjustment) as usize;

                // Recursively propagate up
                self.propagate_checkbox_up(parent_offset, true, cursor_pos);
            }
        } else {
            // When unchecking: uncheck parent if it was checked
            if parent_checked {
                let content_start = parent_offset + 1;
                let content_end = content_start + 1;
                self.buffer
                    .replace(content_start..content_end, " ", *cursor_pos);

                // Remove strikethrough from parent's direct content line
                let parent_line = self.buffer.byte_to_line(parent_offset);
                let adjustment = self.toggle_line_strikethrough(parent_line, false, *cursor_pos);
                *cursor_pos = (*cursor_pos as isize + adjustment) as usize;

                // Recursively propagate up
                self.propagate_checkbox_up(parent_offset, false, cursor_pos);
            }
        }
    }

    /// Propagate checkbox state after tab cycling changes the structure.
    /// Propagate checkbox state after editing (insert/delete).
    /// If current line has a checkbox, propagate from it.
    /// If not, check if we're inside a parent checkbox and re-evaluate it.
    fn propagate_checkbox_after_edit(&mut self) {
        // Fast path: skip all tree traversals if document has no checkboxes
        if !self.buffer.parsed().has_checkboxes {
            return;
        }
        let cursor_offset = self.cursor().offset;
        let line_idx = self.buffer.byte_to_line(cursor_offset);
        let markers = self.buffer.line_markers(line_idx);

        if let Some(is_checked) = markers.checkbox() {
            // Current line has a checkbox - propagate from it
            let line_text = self.buffer.slice_cow(markers.range.clone());
            let checkbox_pattern = if is_checked { "[x]" } else { "[ ]" };
            let alt_pattern = if is_checked { "[X]" } else { "" };

            let checkbox_offset = line_text.find(checkbox_pattern).or_else(|| {
                if !alt_pattern.is_empty() {
                    line_text.find(alt_pattern)
                } else {
                    None
                }
            });

            if let Some(relative_offset) = checkbox_offset {
                let checkbox_byte_start = markers.range.start + relative_offset;
                let mut cursor_pos = cursor_offset;
                self.propagate_checkbox_up(checkbox_byte_start, is_checked, &mut cursor_pos);
                self.selection = Selection::new(cursor_pos, cursor_pos);
            }
        } else {
            // No checkbox on current line - maybe we deleted one.
            // Check if there's a parent checkbox that needs re-evaluation.
            self.propagate_from_parent_checkbox();
        }
    }

    /// When current line has no checkbox, find parent checkbox and re-evaluate it.
    fn propagate_from_parent_checkbox(&mut self) {
        let cursor_offset = self.cursor().offset;

        // Try to find a parent checkbox using tree-sitter.
        // If cursor is at end of file or outside a node, try one position back.
        let parent_info = self.find_parent_checkbox(cursor_offset).or_else(|| {
            if cursor_offset > 0 {
                self.find_parent_checkbox(cursor_offset - 1)
            } else {
                None
            }
        });

        let Some(parent_info) = parent_info else {
            return;
        };

        // Also need to find siblings from a valid position
        let sibling_offset =
            if self.find_sibling_checkboxes(cursor_offset).is_empty() && cursor_offset > 0 {
                cursor_offset - 1
            } else {
                cursor_offset
            };

        let (parent_checkbox_offset, parent_checked) = parent_info;

        // Find siblings using the adjusted offset
        let siblings = self.find_sibling_checkboxes(sibling_offset);

        // If no siblings with checkboxes, nothing to propagate
        if siblings.is_empty() {
            // No sibling checkboxes - if parent was checked, it should stay checked
            // (the deleted item wasn't affecting the parent's state)
            return;
        }

        let all_siblings_checked = siblings.iter().all(|(_, checked)| *checked);
        let mut cursor_pos = cursor_offset;

        if all_siblings_checked && !parent_checked {
            // All remaining siblings are checked, check the parent
            let content_start = parent_checkbox_offset + 1;
            let content_end = content_start + 1;
            self.buffer
                .replace(content_start..content_end, "x", cursor_pos);

            let parent_line = self.buffer.byte_to_line(parent_checkbox_offset);
            let adjustment = self.toggle_line_strikethrough(parent_line, true, cursor_pos);
            cursor_pos = (cursor_pos as isize + adjustment) as usize;

            self.propagate_checkbox_up(parent_checkbox_offset, true, &mut cursor_pos);
            self.selection = Selection::new(cursor_pos, cursor_pos);
        } else if !all_siblings_checked && parent_checked {
            // Some siblings unchecked, uncheck the parent
            let content_start = parent_checkbox_offset + 1;
            let content_end = content_start + 1;
            self.buffer
                .replace(content_start..content_end, " ", cursor_pos);

            let parent_line = self.buffer.byte_to_line(parent_checkbox_offset);
            let adjustment = self.toggle_line_strikethrough(parent_line, false, cursor_pos);
            cursor_pos = (cursor_pos as isize + adjustment) as usize;

            self.propagate_checkbox_up(parent_checkbox_offset, false, &mut cursor_pos);
            self.selection = Selection::new(cursor_pos, cursor_pos);
        }
    }

    /// Add or remove strikethrough (`~~`) from a line's content.
    fn toggle_line_strikethrough(
        &mut self,
        line_idx: usize,
        add_strikethrough: bool,
        cursor_pos: usize,
    ) -> isize {
        // Clear tab cycle cache since content is changing
        self.tab_cycle_cache = None;
        if line_idx >= self.buffer.line_count() {
            return 0;
        }
        let line = self.buffer.line_markers(line_idx);

        let content_start = line.content_start();
        let content_end = line.range.end;

        if content_start >= content_end {
            return 0;
        }

        let content = self.buffer.slice_cow(content_start..content_end);
        let trimmed = content.trim();

        if trimmed.is_empty() {
            return 0;
        }

        if add_strikethrough {
            if trimmed.starts_with("~~") && trimmed.ends_with("~~") {
                return 0;
            }

            let leading_ws = content.len() - content.trim_start().len();
            let trailing_ws = content.len() - content.trim_end().len();

            let text_start = content_start + leading_ws;
            let text_end = content_end - trailing_ws;

            self.buffer.insert(text_end, "~~", cursor_pos);
            self.buffer.insert(text_start, "~~", cursor_pos);

            let mut adjustment: isize = 0;
            if cursor_pos > text_start {
                adjustment += 2;
            }
            if cursor_pos > text_end {
                adjustment += 2;
            }
            adjustment
        } else {
            let leading_ws = content.len() - content.trim_start().len();
            let text_start = content_start + leading_ws;

            if trimmed.starts_with("~~") && trimmed.ends_with("~~") && trimmed.len() >= 4 {
                let trailing_ws = content.len() - content.trim_end().len();
                let text_end = content_end - trailing_ws;

                self.buffer.delete((text_end - 2)..text_end, cursor_pos);
                self.buffer.delete(text_start..(text_start + 2), cursor_pos);

                let mut adjustment: isize = 0;
                if cursor_pos > text_start + 2 {
                    adjustment -= 2;
                }
                if cursor_pos > text_end {
                    adjustment -= 2;
                }
                adjustment
            } else {
                0
            }
        }
    }
}