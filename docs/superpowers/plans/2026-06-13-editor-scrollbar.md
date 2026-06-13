# Editor Scrollbar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a visible scrollbar to the editor for long documents, rendered inline in Editor::render().

**Architecture:** Scrollbar is a child div of the editor root, absolutely positioned on the right. It reads scroll state from ListState (measured item bounds + estimated unmeasured). Mouse events (drag thumb, click track) map to ListState::scroll_by(). Uses GPUI hover() for width expansion.

**Tech Stack:** Rust, GPUI 0.2

**Spec:** `docs/superpowers/specs/2026-06-13-editor-scrollbar-design.md`

**Files:**
- Modify: `src/editor/mod.rs` — scrollbar rendering, mouse handling, state fields, helper methods

---

### Task 1: Add ScrollbarDrag marker and state fields

**Files:** Modify `src/editor/mod.rs`

- [ ] **Step 1: Add ScrollbarDrag marker struct near SelectionDrag**

After `struct SelectionDrag;` (line ~29), add:

```rust
/// Marker type for scrollbar drag operations.
struct ScrollbarDrag;
```

- [ ] **Step 2: Add state fields to Editor struct**

After `is_selecting: bool` (line ~1644), add:

```rust
/// Y coordinate where scrollbar thumb drag started (None when not dragging).
scrollbar_drag_start_y: Option<Pixels>,
```

- [ ] **Step 3: Initialize field in Editor::new()**

After `is_selecting: false` (line ~1707), add:

```rust
scrollbar_drag_start_y: None,
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check 2>&1`
Expected: Compiles successfully (unused field warnings may appear — will be used later).

- [ ] **Step 5: Commit**

```bash
git add src/editor/mod.rs && git commit -m "feat(scrollbar): add ScrollbarDrag marker and state fields"
```

---

### Task 2: Add helper methods for scrollbar geometry

**Files:** Modify `src/editor/mod.rs`

- [ ] **Step 1: Add compute_total_content_height method**

Before `impl Render for Editor`, add to `impl Editor`:

```rust
/// Compute total content height from measured item bounds + estimates for unmeasured.
fn compute_total_content_height(&self) -> f32 {
    let total_lines = self.state.buffer.line_count();
    let default_line_h = self.config.line_height.to_pixels(px(16.0)) // rem assumed ~16px
        .0;

    // Directly use viewport rem_size would be better, but we pass it from render
    // For now, accumulate measured bounds and estimate remainder
    let mut measured_height = 0.0f32;
    let mut measured_count = 0usize;

    for i in 0..total_lines {
        if let Some(bounds) = self.list_state.bounds_for_item(i) {
            measured_height += bounds.size.height.0;
            measured_count += 1;
        }
    }

    let unmeasured = total_lines.saturating_sub(measured_count);
    measured_height + (unmeasured as f32 * default_line_h)
}

/// Compute the scroll offset in pixels from the top of content.
fn compute_scroll_offset_pixels(&self) -> f32 {
    let default_line_h = self.config.line_height.to_pixels(px(16.0)).0;
    let scroll = self.list_state.logical_scroll_top();

    let mut offset = 0.0f32;
    for i in 0..scroll.item_ix {
        if let Some(bounds) = self.list_state.bounds_for_item(i) {
            offset += bounds.size.height.0;
        } else {
            offset += default_line_h;
        }
    }
    offset + (scroll.offset_in_item as f32)
}
```

Note: The px(16.0) is a rough estimate of 1rem. We'll use the actual rem size where `line_height.to_pixels(rem_size)` is available in render. For the helper, we accept a `rem_size: Pixels` parameter.

- [ ] **Step 2: Refine compute_total_content_height to accept rem_size**

Replace Step 1's implementations with:

```rust
fn compute_total_content_height(&self, rem_size: Pixels) -> f32 {
    let total_lines = self.state.buffer.line_count();
    let default_line_h = self.config.line_height.to_pixels(rem_size).0;

    let mut measured_height = 0.0f32;
    let mut measured_count = 0usize;

    for i in 0..total_lines {
        if let Some(bounds) = self.list_state.bounds_for_item(i) {
            measured_height += bounds.size.height.0;
            measured_count += 1;
        }
    }

    let unmeasured = total_lines.saturating_sub(measured_count);
    measured_height + (unmeasured as f32 * default_line_h)
}

fn compute_scroll_offset_pixels(&self, rem_size: Pixels) -> f32 {
    let default_line_h = self.config.line_height.to_pixels(rem_size).0;
    let scroll = self.list_state.logical_scroll_top();

    let mut offset = 0.0f32;
    for i in 0..scroll.item_ix {
        if let Some(bounds) = self.list_state.bounds_for_item(i) {
            offset += bounds.size.height.0;
        } else {
            offset += default_line_h;
        }
    }
    offset + (scroll.offset_in_item as f32)
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check 2>&1`
Expected: Compile, used later.

- [ ] **Step 4: Commit**

```bash
git add src/editor/mod.rs && git commit -m "feat(scrollbar): add content height and scroll offset helpers"
```

---

### Task 3: Implement scrollbar rendering

**Files:** Modify `src/editor/mod.rs`

- [ ] **Step 1: Add render_scrollbar method to Editor**

In `impl Editor`, before `impl Render for Editor`, add:

```rust
/// Build the scrollbar element. Returns None if content fits in viewport.
fn render_scrollbar(
    &mut self,
    theme: &EditorTheme,
    rem_size: Pixels,
    editor_id: usize,
    cx: &mut Context<Self>,
) -> Option<AnyElement> {
    let viewport = self.list_state.viewport_bounds();
    let viewport_h = viewport.size.height.0;
    let total_h = self.compute_total_content_height(rem_size);

    // No scrollbar if content fits in viewport
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

    Some(
        div()
            .id(("scrollbar", editor_id))
            .absolute()
            .right_0()
            .top_0()
            .h_full()
            .w(px(8.0))
            .hover(|d| d.w(px(12.0)))
            .z_index(10)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(
                    move |editor, event: &gpui::MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();

                        let click_y = event.position.y.0;

                        // Check if click is on the thumb
                        if click_y >= thumb_top_val && click_y <= thumb_top_val + thumb_h_val {
                            // Drag start — record initial state
                            editor.scrollbar_drag_start_y = Some(Pixels(click_y));
                        } else {
                            // Track click — page up/down
                            let scroll_y = editor.compute_scroll_offset_pixels(rem_size);
                            let thumb_center = thumb_top_val + thumb_h_val / 2.0;
                            if click_y < thumb_center {
                                // Click above thumb: page up
                                editor.list_state.scroll_by(Pixels(-viewport_h));
                            } else {
                                // Click below thumb: page down
                                editor.list_state.scroll_by(Pixels(viewport_h));
                            }
                        }
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
                    let delta_y_px = mouse_y.0 - start_y.0;

                    let track_range = track_h_val - thumb_h_val;
                    if track_range > 0.0 {
                        let content_delta =
                            (delta_y_px / track_range) * total_h_val;
                        editor.list_state.scroll_by(Pixels(content_delta));
                        editor.scrollbar_drag_start_y = Some(mouse_y);
                        cx.notify();
                    }
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|editor, _event: &gpui::MouseUpEvent, _window, cx| {
                    editor.scrollbar_drag_start_y = None;
                    cx.notify();
                }),
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
```

Imports needed (already present or to add):
- `Pixels` — already imported
- `Rgba`, `Hsla` — already imported via `gpui::*`
- `CursorStyle` — already imported
- `DragMoveEvent` — already imported
- `MouseButton` — already imported
- `px` — already imported

- [ ] **Step 2: Wire scrollbar into Editor::render()**

In the render method, before the return statement, capture the scrollbar:

After computing `line_theme` (around line 3420), compute `scrollbar_element`:

```rust
let scrollbar_element = self.render_scrollbar(&theme, font_size, editor_id, cx);
```

Then change the final chain from:

```rust
.child(line_list)
.children(self.render_autocomplete(&line_theme, window, cx))
```

to:

```rust
.child(line_list)
.children(scrollbar_element)
.children(self.render_autocomplete(&line_theme, window, cx))
```

Note: The `render_autocomplete` uses `rem_size` from `window.rem_size()`, and we already have `font_size` computed as `text_style.font_size.to_pixels(window.rem_size())`. We should use `window.rem_size()` for `rem_size` in `render_scrollbar` since that's the actual 1rem value.

Actually, looking at the code, `font_size` is a Pixels value computed from the text style's font size. The line height is defined in rems, and we need rem_size. Let me get it cleanly:

```rust
let rem_size = window.rem_size();
```

Use this instead of font_size in render_scrollbar.

- [ ] **Step 3: Verify compilation**

Run: `cargo check 2>&1`
Expected: Compiles.

- [ ] **Step 4: Commit**

```bash
git add src/editor/mod.rs && git commit -m "feat(scrollbar): implement scrollbar rendering and interaction"
```

---

### Task 4: Build and smoke test

- [ ] **Step 1: Full build**

Run: `cargo build 2>&1`
Expected: Builds successfully.

- [ ] **Step 2: Run unit tests**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 3: Commit final**

```bash
git add -A && git commit -m "refactor(scrollbar): final cleanup after scrollbar implementation"
```
