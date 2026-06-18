# Find & Replace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add VSCode-style find and replace to rustmd — overlay search bar, match highlighting, and single/all replacement.

**Architecture:** `FindState` in `src/editor/find.rs` owns search/replace state and logic (no GPUI dependency). Editor stores `find_state: Option<FindState>`. The search bar renders as an absolute-positioned overlay in `editor/render.rs`. Match highlighting reuses the existing `inline_highlight_ranges` mechanism in `Line`. Search uses `regex::escape` + optional `(?i)` flag (regex crate already in deps).

**Tech Stack:** Rust, GPUI, regex crate

---

### Task 1: Define Find/Replace GPUI actions

**Files:**
- Modify: `src/editor/action.rs`
- Modify: `src/editor/mod.rs`

- [ ] **Step 1: Add action structs to `src/editor/action.rs`**

Append to the end of `src/editor/action.rs`, after `pub struct CenterLine;`:

```rust
#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct ToggleFind;

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct FindNext;

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct FindPrevious;

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct ReplaceNext;

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct ReplaceAll;
```

- [ ] **Step 2: Export new actions from `src/editor/mod.rs`**

Find the existing `pub use action::...` line (line 6). Change:

```rust
pub use action::{CenterLine, Direction, DispatchEditorAction, EditorAction};
```

To:

```rust
pub use action::{
    CenterLine, Direction, DispatchEditorAction, EditorAction, FindNext, FindPrevious, ReplaceAll,
    ReplaceNext, ToggleFind,
};
```

- [ ] **Step 3: Run `cargo check`**

Run: `cargo check`
Expected: Success.

- [ ] **Step 4: Commit**

Run:
```
git add -A
git commit -m "feat: add find/replace action types"
```

---

### Task 2: Create `src/editor/find.rs` — FindState + search/replace logic

**Files:**
- Create: `src/editor/find.rs`

- [ ] **Step 1: Write `src/editor/find.rs`**

```rust
use std::ops::Range;

/// State for the find-and-replace search bar.
/// Pure data + algorithms — no GPUI dependency.
pub struct FindState {
    pub visible: bool,
    pub query: String,
    pub replace_text: String,
    pub matches: Vec<Range<usize>>,
    pub current_match: Option<usize>,
    pub match_case: bool,
    pub replace_visible: bool,
    /// When true, keyboard input goes to the search/replace input instead of the editor.
    pub input_focused: bool,
    /// When true, the replace input field is focused (else search input).
    pub replace_input_focused: bool,
}

impl FindState {
    pub fn new() -> Self {
        Self {
            visible: false,
            query: String::new(),
            replace_text: String::new(),
            matches: Vec::new(),
            current_match: None,
            match_case: false,
            replace_visible: false,
            input_focused: false,
            replace_input_focused: false,
        }
    }

    /// Search the full text for all matches of the current query.
    /// Called whenever the query or match_case changes.
    pub fn search(&mut self, text: &str) {
        self.matches.clear();
        self.current_match = None;
        if self.query.is_empty() {
            return;
        }
        let pattern = regex::escape(&self.query);
        let re = if self.match_case {
            regex::Regex::new(&pattern).unwrap()
        } else {
            regex::Regex::new(&format!("(?i){}", pattern)).unwrap()
        };
        self.matches = re.find_iter(text).map(|m| m.range()).collect();
        if !self.matches.is_empty() {
            self.current_match = Some(0);
        }
    }

    /// Move to the next match. Wraps around.
    pub fn find_next(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let next = match self.current_match {
            Some(i) => (i + 1) % self.matches.len(),
            None => 0,
        };
        self.current_match = Some(next);
        Some(next)
    }

    /// Move to the previous match. Wraps around.
    pub fn find_prev(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let prev = match self.current_match {
            Some(i) => {
                if i == 0 {
                    self.matches.len() - 1
                } else {
                    i - 1
                }
            }
            None => self.matches.len() - 1,
        };
        self.current_match = Some(prev);
        Some(prev)
    }

    /// Number of matches found.
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// Reset all state (close the bar).
    pub fn close(&mut self) {
        self.visible = false;
        self.query.clear();
        self.replace_text.clear();
        self.matches.clear();
        self.current_match = None;
        self.input_focused = false;
        self.replace_input_focused = false;
        self.replace_visible = false;
    }

    /// Returns the byte range of the current match, if any.
    pub fn current_match_range(&self) -> Option<Range<usize>> {
        self.current_match.map(|i| self.matches[i].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_query_finds_nothing() {
        let mut fs = FindState::new();
        fs.search("hello world");
        assert!(fs.matches.is_empty());
        assert_eq!(fs.match_count(), 0);
    }

    #[test]
    fn test_basic_search() {
        let mut fs = FindState::new();
        fs.query = "hello".to_string();
        fs.search("hello world hello");
        assert_eq!(fs.match_count(), 2);
        assert_eq!(fs.matches[0], 0..5);
        assert_eq!(fs.matches[1], 12..17);
        assert_eq!(fs.current_match, Some(0));
    }

    #[test]
    fn test_case_sensitive() {
        let mut fs = FindState::new();
        fs.query = "Hello".to_string();
        fs.match_case = true;
        fs.search("hello Hello HELLO");
        assert_eq!(fs.match_count(), 1);
        assert_eq!(fs.matches[0], 6..11);
    }

    #[test]
    fn test_case_insensitive() {
        let mut fs = FindState::new();
        fs.query = "hello".to_string();
        fs.match_case = false;
        fs.search("hello Hello HELLO");
        assert_eq!(fs.match_count(), 3);
    }

    #[test]
    fn test_find_next_wraps() {
        let mut fs = FindState::new();
        fs.query = "a".to_string();
        fs.search("a b a");
        assert_eq!(fs.match_count(), 2);
        assert_eq!(fs.current_match, Some(0));
        assert_eq!(fs.find_next(), Some(1));
        assert_eq!(fs.find_next(), Some(0));
    }

    #[test]
    fn test_find_prev_wraps() {
        let mut fs = FindState::new();
        fs.query = "a".to_string();
        fs.search("a b a");
        assert_eq!(fs.current_match, Some(0));
        assert_eq!(fs.find_prev(), Some(1));
        assert_eq!(fs.find_prev(), Some(0));
    }

    #[test]
    fn test_close_resets_state() {
        let mut fs = FindState::new();
        fs.visible = true;
        fs.query = "test".to_string();
        fs.replace_text = "new".to_string();
        fs.search("test test");
        assert!(fs.match_count() > 0);
        fs.close();
        assert!(!fs.visible);
        assert!(fs.query.is_empty());
        assert!(fs.matches.is_empty());
        assert_eq!(fs.match_count(), 0);
    }

    #[test]
    fn test_search_with_regex_special_chars() {
        let mut fs = FindState::new();
        fs.query = "(a+b)".to_string();
        fs.search("(a+b) test (a+b)");
        assert_eq!(fs.match_count(), 2);
    }

    #[test]
    fn test_current_match_range() {
        let mut fs = FindState::new();
        assert!(fs.current_match_range().is_none());
        fs.query = "x".to_string();
        fs.search("x y x");
        assert_eq!(fs.current_match_range(), Some(0..1));
        fs.find_next();
        assert_eq!(fs.current_match_range(), Some(4..5));
    }
}
```

- [ ] **Step 2: Register the module in `src/editor/mod.rs`**

Add after the existing `mod persistence;` (around line 57):

```rust
pub(crate) mod find;
```

- [ ] **Step 3: Run `cargo test`**

Run: `cargo test`
Expected: 274 existing + 9 new = 283 tests pass.

- [ ] **Step 4: Commit**

Run:
```
git add -A
git commit -m "feat: add FindState with search/replace logic and tests"
```

---

### Task 3: Integrate FindState into Editor — field + keyboard routing

**Files:**
- Modify: `src/editor/mod.rs`

- [ ] **Step 1: Add `find_state` field to Editor struct**

In `src/editor/mod.rs`, add to the Editor struct fields (after `cursor_blink_visible: bool,`):

```rust
    find_state: Option<find::FindState>,
```

- [ ] **Step 2: Initialize in `with_config`**

In the `Self { ... }` init block inside `with_config`, add after `cursor_blink_visible: true,`:

```rust
            find_state: None,
```

- [ ] **Step 3: Add key-routing at top of `on_key_down`**

In `fn on_key_down(...)`, right after `self.reset_cursor_blink();` (around line 961), BEFORE the IME marked range block:

```rust
        // Route keyboard input to find bar when focused
        if let Some(ref mut fs) = self.find_state {
            if fs.visible && fs.input_focused {
                self.handle_find_key(event, window, cx);
                return;
            }
        }
```

- [ ] **Step 4: Add `handle_find_key` method**

Insert this new method in an `impl Editor` block — put it right before `on_modifiers_changed` (around line 1210):

```rust
    /// Handle keyboard events when the find bar has input focus.
    fn handle_find_key(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let fs = self.find_state.as_mut().unwrap();

        match keystroke.key.as_str() {
            "escape" => {
                fs.close();
            }
            "enter" if !keystroke.modifiers.shift => {
                if let Some(idx) = fs.find_next() {
                    let range = fs.matches[idx].clone();
                    self.state.selection = crate::cursor::Selection::new(range.start, range.end);
                    self.scroll_to_cursor_pending = true;
                }
            }
            "enter" => {
                // Shift+Enter: previous match
                if let Some(idx) = fs.find_prev() {
                    let range = fs.matches[idx].clone();
                    self.state.selection = crate::cursor::Selection::new(range.start, range.end);
                    self.scroll_to_cursor_pending = true;
                }
            }
            "backspace" => {
                if fs.replace_input_focused {
                    fs.replace_text.pop();
                } else {
                    fs.query.pop();
                    let text = self.state.buffer.text();
                    fs.search(&text);
                    if let Some(idx) = fs.current_match {
                        let range = fs.matches[idx].clone();
                        self.state.selection =
                            crate::cursor::Selection::new(range.start, range.end);
                        self.scroll_to_cursor_pending = true;
                    }
                }
            }
            "tab" => {
                if fs.replace_visible {
                    fs.replace_input_focused = !fs.replace_input_focused;
                }
                // If replace not visible, stay in search input
            }
            _ => {
                if let Some(key_char) = &keystroke.key_char {
                    if fs.replace_input_focused {
                        fs.replace_text.push_str(key_char);
                    } else {
                        fs.query.push_str(key_char);
                        let text = self.state.buffer.text();
                        fs.search(&text);
                        if let Some(idx) = fs.current_match {
                            let range = fs.matches[idx].clone();
                            self.state.selection =
                                crate::cursor::Selection::new(range.start, range.end);
                            self.scroll_to_cursor_pending = true;
                        }
                    }
                }
            }
        }
        cx.notify();
    }
```

- [ ] **Step 5: Run `cargo check`**

Run: `cargo check`
Expected: Success.

- [ ] **Step 6: Commit**

Run:
```
git add -A
git commit -m "feat: integrate FindState into Editor with keyboard routing"
```

---

### Task 4: Render find bar UI + match highlighting in `render.rs`

**Files:**
- Modify: `src/editor/render.rs`
- Modify: `src/editor/find.rs` (add `find_match_highlights` helper)

- [ ] **Step 1: Add a helper to get line-level match highlights**

Add this method to `impl FindState` in `src/editor/find.rs`:

```rust
    /// For a given line byte range, return (inline_highlight_ranges, current_match_range)
    /// where ranges are relative to the line start.
    /// `current_match_range` is the single active match range (if on this line).
    pub fn highlights_for_line(
        &self,
        line_start: usize,
        line_end: usize,
    ) -> (Vec<Range<usize>>, Option<Range<usize>>) {
        if self.matches.is_empty() || self.query.is_empty() {
            return (Vec::new(), None);
        }
        let mut highlights = Vec::new();
        let mut current = None;
        for (i, m) in self.matches.iter().enumerate() {
            if m.start >= line_end || m.end <= line_start {
                continue;
            }
            let rel_start = m.start.saturating_sub(line_start);
            let rel_end = m.end.saturating_sub(line_start);
            let rel_end = rel_end.min(line_end - line_start);
            if rel_start < rel_end {
                highlights.push(rel_start..rel_end);
            }
            if Some(i) == self.current_match {
                current = Some(rel_start..rel_end);
            }
        }
        (highlights, current)
    }
```

- [ ] **Step 2: Inject match highlights in `build_line` in `editor/render.rs`**

Find the `build_line` inner closure inside the `list(...)` call (around line 233). Look for the two main-line `build_line` calls — the one with `inline_highlight_ranges: Vec::new()` and `inline_highlight_color = None` (around lines 302-312).

Replace those lines. Find this block:

```rust
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
```

Change it to inject find match highlights:

```rust
                // Inject find match highlights if search is active
                let find_highlights = self.find_state.as_ref().and_then(|fs| {
                    if !fs.visible || fs.matches.is_empty() { None }
                    else {
                        let (highlights, current) = fs.highlights_for_line(
                            line_byte_range.start,
                            line_byte_range.end,
                        );
                        Some((highlights, current))
                    }
                });
                let (inline_highlight_ranges, inline_highlight_color) =
                    if let Some((ranges, current_range)) = find_highlights {
                        let mut all_ranges = ranges.clone();
                        let bg = {
                            let mut c: gpui::Hsla = theme.orange.into();
                            c.a = 0.25;
                            gpui::Rgba::from(c)
                        };
                        // If current match is on this line and not already in all_ranges, add it
                        if let Some(cur) = &current_range {
                            if !all_ranges.contains(cur) {
                                all_ranges.push(cur.clone());
                            }
                        }
                        (all_ranges, Some(bg))
                    } else {
                        (Vec::new(), None)
                    };

                // Build the main line element
                let line_element = build_line(
                    &snapshot,
                    ix,
                    extra_styles,
                    line_bg,
                    inline_highlight_ranges,
                    inline_highlight_color,
```

The variable `line_byte_range` is not defined inline there. Let me check what variables are in scope in the list callback.

Looking at the list callback (around line 220-325), the closure has `ix` as the line index. I need to get the byte range for the line. The snapshot has `line_byte_range(ix)` but that requires `&self` access. Actually in the list callback, we're inside `move |ix, _window, _cx|` which captures references. The `snapshot` variable is the `RenderSnapshot` captured by the closure.

Let me check if `snapshot.line_byte_range(ix)` is public... Looking at buffer.rs line 63:

```rust
fn line_byte_range(&self, line_idx: usize) -> Range<usize> {
    compute_line_byte_range(&self.rope, line_idx)
}
```

It's not `pub`. But we can compute it inline using `compute_line_byte_range` from the rope, or we can add a public method. Actually, easier: use `snapshot.rope` directly.

Or I can compute byte range using the existing line_markers which gives us `line.range`.

Let me think about this differently. In the list callback, we're already calling `snapshot.line_markers(ix)` which gives us `LineMarkers { range, ... }`. That range IS the byte range. Let me check...

Looking at render.rs lines 242-243:
```rust
let line_markers = snap.line_markers(line_idx);
```

And `line_markers.range` is the byte range. So we can use `line_markers.range.start` and `line_markers.range.end`.

So the code should be:

```rust
let line_markers = snap.line_markers(line_idx);
let line_byte_range = line_markers.range.clone();

// ... then later ...
let find_highlights = ... fs.highlights_for_line(line_byte_range.start, line_byte_range.end);
```

But wait, `inline_highlight_ranges` is already defined in the `build_line` params. The ranges need to be relative to the line start. `line_byte_range.start` is the byte offset of the start of the line. And `highlights_for_line` already subtracts `line_start`. So that should work.

Let me fix the code to be clearer. I'll declare the byte range before calling build_line.

Actually, looking more carefully at the code in render.rs, `build_line` is defined as a closure INSIDE the list closure. So I can add the find highlight logic right before the `build_line` call (around line 302-312).

Let me rewrite this step more precisely.

The existing code at ~line 300-325:

```rust
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
```

Replace with:

```rust
                // Inject find match highlights if search is active
                let (inline_highlight_ranges, inline_highlight_color) =
                    if let Some(ref fs) = self.find_state
                        && fs.visible
                        && !fs.matches.is_empty()
                        && !fs.query.is_empty()
                    {
                        let (highlights, current) = fs.highlights_for_line(
                            line_markers.range.start,
                            line_markers.range.end,
                        );
                        let mut all_ranges = highlights;
                        // Current match should always be highlighted
                        if let Some(cur) = current {
                            if !all_ranges.contains(&cur) {
                                all_ranges.push(cur);
                            }
                        }
                        let bg = {
                            let mut c: gpui::Hsla = theme.orange.into();
                            c.a = 0.25;
                            gpui::Rgba::from(c)
                        };
                        (all_ranges, Some(bg))
                    } else {
                        (Vec::new(), None)
                    };

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
```

Wait, but `self` is not available in the list callback closure. Let me look at the closure more carefully.

The list closure: `move |ix, _window, _cx| { ... }`

It captures by move. The surrounding context has `self` (the Editor), `snapshot`, `theme`, `line_theme_for_list`, etc.

Actually, in GPUI's `list()` function, the closure captures variables from the rendering context. Since this is inside `fn render(&mut self, ...)`, `self` is available as a reference. But the closure uses `move` which captures owned values.

Looking at what's captured:
```rust
let line_list = div().id(("line-list", editor_id)).size_full().child(
    list(self.list_state.clone(), move |ix, _window, _cx| {
        // ... uses snapshot, theme, etc. which are captured by move
        // ... uses line_markers which is defined inside the closure
    })
);
```

So `self` is NOT available inside the list closure. But the variables captured before it are. I need to extract the find state before creating the list closure.

I can read `self.find_state` before the `list()` call and pass it in:

```rust
let find_state = self.find_state.as_ref().map(|fs| {
    // We need to check visibility and query status
    (fs.visible, fs.query.clone(), fs.matches.clone(), fs.current_match, fs.match_case)
});
```

But that's messy with cloning. A cleaner approach is to precompute the match data needed for highlighting before the list closure.

Let me think of a better approach. I can create a `FindHighlightData` struct before the list closure:

```rust
struct FindHighlightData {
    visible: bool,
    query_empty: bool,
    /// For each line index, the (all_matches, current_match) tuple
    /// where ranges are (start_offset, end_offset) relative to line start
    highlights: Vec<(Vec<Range<usize>>, Option<Range<usize>>)>,
}
```

But computing highlights for ALL lines upfront would be wasteful. Most lines won't have matches.

Actually, the simplest approach: pre-filter which lines have matches. Since `FindState::matches` contains byte ranges, I can create a mapping from line number to match indices.

But honestly, since the list callback is `move |ix, ...|`, I can pass in the find state's visible / matches data and compute on the fly. The issue is that `line_markers` (which gives us the byte range) is computed inside the closure.

Actually, the byte range is `line_markers.range` which comes from `snap.line_markers(line_idx)`. Inside the list closure, `snap` is the captured `snapshot`. So I have the byte range.

Let me pass the find state data before the closure. Here's the approach:

Before the `list()` call, extract what we need:

```rust
// Pre-extract find match data for highlighting
let find_visible = self.find_state.as_ref().map_or(false, |fs| fs.visible && !fs.query.is_empty());
let find_matches = self.find_state.as_ref().map(|fs| fs.matches.clone()).unwrap_or_default();
let find_current_match = self.find_state.as_ref().and_then(|fs| fs.current_match);
```

Then inside the list closure, compute highlights from these:

```rust
let (inline_highlight_ranges, inline_highlight_color) =
    if find_visible && !find_matches.is_empty() {
        let line_range = line_markers.range.clone();
        let mut ranges: Vec<Range<usize>> = Vec::new();
        let mut current: Option<Range<usize>> = None;
        for (i, m) in find_matches.iter().enumerate() {
            if m.start >= line_range.end || m.end <= line_range.start {
                continue;
            }
            let rel = (m.start.saturating_sub(line_range.start))..(m.end.saturating_sub(line_range.start));
            if Some(i) == find_current_match {
                current = Some(rel.clone());
            }
            ranges.push(rel);
        }
        let bg = {
            let mut c: gpui::Hsla = theme.orange.into();
            c.a = 0.25;
            gpui::Rgba::from(c)
        };
        (ranges, Some(bg))
    } else {
        (Vec::new(), None)
    };
```

This clones `find_matches` which is a `Vec<Range<usize>>` — typically small. Acceptable for render path.

OK, let me update the plan with this approach. It's cleaner.

- [ ] **Step 3: Add find bar UI rendering to the Editor's render chain**

In `editor/render.rs`, find the end of the `impl Render for Editor` block (around line 679). Add `.children(self.render_find_bar(&theme, cx))` at the end of the builder chain, after the autocomplete:

```rust
            .child(line_list)
            .children(self.render_scrollbar(&theme, window.rem_size(), editor_id, cx))
            .children(self.render_autocomplete(&line_theme, window, cx))
            .children(self.render_find_bar(&theme, cx))
```

- [ ] **Step 4: Implement `render_find_bar` method**

Add this method to `impl Editor` (in the impl block at line 682+, after `render_autocomplete`):

```rust
    /// Render the find/replace bar as an overlay at the top of the editor.
    fn render_find_bar(
        &self,
        theme: &EditorTheme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let fs = self.find_state.as_ref()?;
        if !fs.visible {
            return None;
        }

        let bar_bg = {
            let mut c: gpui::Hsla = theme.background.into();
            c.a = 0.95;
            gpui::Rgba::from(c)
        };

        let input_bg = {
            let mut c: gpui::Hsla = theme.selection.into();
            c.a = 0.4;
            gpui::Rgba::from(c)
        };

        let has_results = !fs.matches.is_empty() || fs.query.is_empty();
        let match_info = if fs.query.is_empty() {
            String::new()
        } else {
            format!(
                "{}/{}",
                fs.current_match.map_or(0, |i| i + 1),
                fs.matches.len()
            )
        };

        let query_display: gpui::SharedString = if fs.query.is_empty() {
            "Search\u{2026}".into()
        } else {
            fs.query.clone().into()
        };

        let replace_display: gpui::SharedString = fs.replace_text.clone().into();

        let border_color = if !has_results && !fs.query.is_empty() {
            theme.red
        } else {
            theme.comment
        };

        let find_bar = div()
            .id("find-bar")
            .absolute()
            .top(px(0.0))
            .right(px(4.0))
            .w(px(360.0))
            .bg(bar_bg)
            .border_1()
            .border_color(border_color)
            .rounded(px(4.0))
            .py(px(4.0))
            .px(px(8.0))
            .shadow_lg()
            .text_size(px(13.0))
            .font(gpui::font("Segoe UI"))
            // Search row
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.0))
                    .child(div().text_color(theme.comment).child("\u{1F50D}"))
                    .child(
                        div()
                            .id("find-input")
                            .flex_1()
                            .min_w(px(100.0))
                            .px(px(6.0))
                            .py(px(2.0))
                            .bg(input_bg)
                            .rounded(px(3.0))
                            .text_color(theme.foreground)
                            .child(query_display)
                            .cursor_text()
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(
                                |editor, _event, _window, cx| {
                                    if let Some(ref mut fs) = editor.find_state {
                                        fs.input_focused = true;
                                        fs.replace_input_focused = false;
                                        cx.notify();
                                    }
                                },
                            )),
                    )
                    .child(
                        div().text_color(theme.comment).text_xs().child(match_info),
                    )
                    .child(
                        div()
                            .px(px(4.0))
                            .py(px(2.0))
                            .text_color(theme.foreground)
                            .hover(|d| d.bg(theme.selection))
                            .rounded(px(3.0))
                            .cursor_pointer()
                            .child("\u{25B2}")
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(
                                |editor, _event, window, cx| {
                                    window.dispatch_action(FindPrevious.boxed_clone(), cx);
                                },
                            )),
                    )
                    .child(
                        div()
                            .px(px(4.0))
                            .py(px(2.0))
                            .text_color(theme.foreground)
                            .hover(|d| d.bg(theme.selection))
                            .rounded(px(3.0))
                            .cursor_pointer()
                            .child("\u{25BC}")
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(
                                |editor, _event, window, cx| {
                                    window.dispatch_action(FindNext.boxed_clone(), cx);
                                },
                            )),
                    )
                    .child(
                        div()
                            .px(px(4.0))
                            .py(px(2.0))
                            .text_color(theme.foreground)
                            .hover(|d| d.bg(theme.selection))
                            .rounded(px(3.0))
                            .cursor_pointer()
                            .child("\u{2715}")
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(
                                |editor, _event, window, cx| {
                                    window.dispatch_action(ToggleFind.boxed_clone(), cx);
                                },
                            )),
                    ),
            );

        // If replace_visible, add replace row and buttons
        let find_bar = if fs.replace_visible {
            find_bar.child(
                div()
                    .mt(px(4.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.0))
                    .child(div().w(px(16.0))) // spacer to align with icon
                    .child(
                        div()
                            .id("replace-input")
                            .flex_1()
                            .min_w(px(100.0))
                            .px(px(6.0))
                            .py(px(2.0))
                            .bg(input_bg)
                            .rounded(px(3.0))
                            .text_color(theme.foreground)
                            .child(replace_display)
                            .cursor_text()
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(
                                |editor, _event, _window, cx| {
                                    if let Some(ref mut fs) = editor.find_state {
                                        fs.input_focused = true;
                                        fs.replace_input_focused = true;
                                        cx.notify();
                                    }
                                },
                            )),
                    )
                    .child(
                        div()
                            .px(px(4.0))
                            .py(px(2.0))
                            .text_color(theme.foreground)
                            .hover(|d| d.bg(theme.selection))
                            .rounded(px(3.0))
                            .cursor_pointer()
                            .child("\u{21BB}") // ↻ replace one
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(
                                |editor, _event, window, cx| {
                                    window.dispatch_action(ReplaceNext.boxed_clone(), cx);
                                },
                            )),
                    )
                    .child(
                        div()
                            .px(px(4.0))
                            .py(px(2.0))
                            .text_color(theme.foreground)
                            .hover(|d| d.bg(theme.selection))
                            .rounded(px(3.0))
                            .cursor_pointer()
                            .child("\u{29BF}") // ⦿ replace all
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(
                                |editor, _event, window, cx| {
                                    window.dispatch_action(ReplaceAll.boxed_clone(), cx);
                                },
                            )),
                    ),
            )
        } else {
            find_bar.child(
                div()
                    .mt(px(4.0))
                    .child(
                        div()
                            .text_color(theme.comment)
                            .text_xs()
                            .cursor_pointer()
                            .hover(|d| d.text_color(theme.foreground))
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(
                                |editor, _event, _window, cx| {
                                    if let Some(ref mut fs) = editor.find_state {
                                        fs.replace_visible = true;
                                        cx.notify();
                                    }
                                },
                            ))
                            .child("Replace"),
                    ),
            )
        };

        Some(find_bar.into_any_element())
    }
```

Wait, I realize `FindPrevious`, `FindNext`, `ReplaceNext`, `ReplaceAll`, `ToggleFind` need to be imported. They're in the `action` module and already re-exported from `editor/mod.rs`. Let me add `use super::*;` to bring them in.

Actually, looking at render.rs line 21: `use super::*;` — this already imports everything from the parent module (editor/mod.rs), which includes the action exports. So `ToggleFind`, `FindNext`, etc. should be available.

- [ ] **Step 5: Add `.on_action()` handlers for find actions**

Add these action handlers to the editor's main div in `render.rs`. Add them alongside the existing `on_action` handlers (after the `CenterLine` handler around line 520):

```rust
            .on_action(cx.listener(
                |editor: &mut Editor, _: &ToggleFind, _window, cx| {
                    if let Some(ref mut fs) = editor.find_state {
                        if fs.visible {
                            fs.close();
                        } else {
                            fs.visible = true;
                            fs.input_focused = true;
                        }
                    } else {
                        let mut fs = crate::editor::find::FindState::new();
                        fs.visible = true;
                        fs.input_focused = true;
                        editor.find_state = Some(fs);
                    }
                    cx.notify();
                },
            ))
            .on_action(cx.listener(
                |editor: &mut Editor, _: &FindNext, _window, cx| {
                    if let Some(ref mut fs) = editor.find_state
                        && !fs.matches.is_empty()
                    {
                        if let Some(idx) = fs.find_next() {
                            let range = fs.matches[idx].clone();
                            editor.state.selection =
                                crate::cursor::Selection::new(range.start, range.end);
                            editor.scroll_to_cursor_pending = true;
                            cx.notify();
                        }
                    }
                },
            ))
            .on_action(cx.listener(
                |editor: &mut Editor, _: &FindPrevious, _window, cx| {
                    if let Some(ref mut fs) = editor.find_state
                        && !fs.matches.is_empty()
                    {
                        if let Some(idx) = fs.find_prev() {
                            let range = fs.matches[idx].clone();
                            editor.state.selection =
                                crate::cursor::Selection::new(range.start, range.end);
                            editor.scroll_to_cursor_pending = true;
                            cx.notify();
                        }
                    }
                },
            ))
            .on_action(cx.listener(
                |editor: &mut Editor, _: &ReplaceNext, _window, cx| {
                    if let Some(ref mut fs) = editor.find_state
                        && let Some(range) = fs.current_match_range()
                    {
                        let (start, end) = (range.start, range.end);
                        let _ = editor.state.buffer.edit(start..end, &fs.replace_text);
                        let text = editor.state.buffer.text();
                        fs.search(&text);
                        if let Some(idx) = fs.current_match {
                            let r = fs.matches[idx].clone();
                            editor.state.selection =
                                crate::cursor::Selection::new(r.start, r.end);
                        }
                        cx.notify();
                    }
                },
            ))
            .on_action(cx.listener(
                |editor: &mut Editor, _: &ReplaceAll, _window, cx| {
                    if let Some(ref mut fs) = editor.find_state
                        && !fs.matches.is_empty()
                    {
                        let matches = fs.matches.clone();
                        let replace_text = fs.replace_text.clone();
                        // Replace from end to start to preserve offsets
                        for range in matches.iter().rev() {
                            let _ = editor.state.buffer.edit(range.start..range.end, &replace_text);
                        }
                        let text = editor.state.buffer.text();
                        fs.search(&text);
                        cx.notify();
                    }
                },
            ))
```

Also need to add `FindNext`, `FindPrevious`, `ReplaceNext`, `ReplaceAll` to the imports that `use super::*;` already provides. Since they're re-exported from `src/editor/mod.rs`, they should be accessible.

But wait — `ToggleFind` is NOT imported via the current `pub use` line. I need to add it. Let me check the current exports.

Actually, looking at step 1 of Task 1, I added all five new actions to the `pub use` line in `src/editor/mod.rs`. So they're all available via `use super::*;` in render.rs.

And in the render.rs handlers, I reference `crate::editor::find::FindState` explicitly, which avoids import issues.

- [ ] **Step 6: Run `cargo check`**

Run: `cargo check`
Expected: Success.

- [ ] **Step 7: Commit**

Run:
```
git add -A
git commit -m "feat: render find bar UI and match highlighting"
```

---

### Task 5: Add 🔍 toolbar button

**Files:**
- Modify: `src/menu.rs`

- [ ] **Step 1: Add import and button**

In `src/menu.rs`, add `ToggleFind` to the editor imports:

```rust
use crate::editor::{CenterLine, Editor, EditorConfig, EditorTheme};
```

Change to:

```rust
use crate::editor::{
    CenterLine, Editor, EditorConfig, EditorTheme, ToggleFind,
};
```

In `get_toolbar_buttons()`, add the 🔍 button between Save and NewWindow:

```rust
        ToolbarButton::new("\u{1F4BE}", "Save", Save),
        // ── add this: ──
        ToolbarButton::new("\u{1F50D}", "Find & Replace", ToggleFind),
        // ── end add ──
        ToolbarButton::new("\u{1F532}", "New window", NewWindow),
```

- [ ] **Step 2: Add tooltip for the new button**

In `tooltip_text()`, add a match arm:

```rust
        "Find & Replace" => "Find and replace".into(),
```

- [ ] **Step 3: Update separator logic**

The separator between Save group and NewWindow uses `index == 4`. With the new button at index 4 (0-indexed: About=0, New file=1, Open file=2, Save=3, 🔍=4), the separator should move to `index == 5`:

```rust
        // Separator between icon group and NewWindow
        if index == 5 {
```

- [ ] **Step 4: Run `cargo check`**

Run: `cargo check`
Expected: Success.

- [ ] **Step 5: Commit**

Run:
```
git add -A
git commit -m "feat: add find toolbar button"
```

---

### Task 6: Final integration — verify everything works

**Files:**
- No file changes — just verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass (283+).

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --all-targets 2>&1`
Expected: No new warnings (accept any pre-existing ones).

- [ ] **Step 3: Build release**

Run: `cargo build --release`
Expected: Build succeeds.

- [ ] **Step 4: Final commit**

```bash
git add -A && git commit -m "feat: add find and replace feature"
```
