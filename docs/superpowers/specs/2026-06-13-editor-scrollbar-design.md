# Editor Scrollbar Design

## Overview

Add a visual scrollbar to the markdown editor, displayed on the right side of the
editor area, for scrolling through long documents.

## Visibility

The scrollbar is rendered whenever content exceeds the viewport height. When
content fits within the viewport, the scrollbar is hidden.

## Visual Style

| Property | Value |
|----------|-------|
| Track width | 8px (12px on hover) |
| Corner radius | 4px |
| Track color | `theme.comment` @ 15% opacity |
| Thumb color | `theme.comment` @ 40% opacity |
| Thumb hover | `theme.comment` @ 60% opacity |
| Min thumb height | 20px |
| Transition | width 100ms ease |

Colors are derived from the existing `EditorTheme::comment` (`#6272A4`) at
various alpha levels. No new theme properties needed.

## Interaction

- **Drag thumb**: Scroll proportionally. Thumb follows mouse Y delta mapped to
  content scroll delta.
- **Click track**: Page up/down by one viewport height.
- **Hover**: Track and thumb become more visible; track widens to 12px for
  easier targeting.

No arrow buttons, no middle-click jump-to.

## Architecture

The scrollbar is rendered inline within `Editor::render()` as a child of the
editor's root `div()`, alongside the existing `list()` element:

```
div()                           // Editor root
  .size_full()
  .child(list(...))             // Existing line list (ListState)
  .child(scrollbar_element)     // New: positioned on the right
  .children(autocomplete_popup) // Existing
```

The scrollbar is positioned `absolute`, right-aligned, full height, with the
track and thumb rendered inside. It sits on top of the line content (z-order),
but the editor's content is constrained by `max_line_width` (default 800px) and
`padding_x`, so the 8-12px scrollbar covers only the right margin, not text.

## Data Flow

The scrollbar reads directly from `self.list_state`:

```
total_content_height = Σ measured_item_heights + unmeasured_count × default_line_height
thumb_height = max(track_height × viewport_height / total_height, 20px)
thumb_offset = track_height × scroll_top / total_height
```

- **Measured items**: iterate `list_state.bounds_for_item(i)` for
  `Some(bounds)`, sum `bounds.size.height`.
- **Unmeasured items**: `(total_lines - measured_count) × default_line_height`
  where default is `EditorConfig.line_height` in pixels.
- **scroll_top**: from `list_state.logical_scroll_top()`.
- **viewport_height**: from `list_state.viewport_bounds().size.height`.

On drag, the delta is:
```
scroll_delta = (mouse_delta_y / (track_height - thumb_height)) × total_content_height
```

Compatibility with `CenterLine` (which adds `viewport_h / 2` padding to the last
line): that padding is included in the last measured item's bounds, so the
scrollbar naturally accounts for it. No special handling needed.

## State

Two transient booleans on the `Editor` struct:

```rust
scrollbar_hovered: bool   // Reset each render based on mouse position
scrollbar_dragging: bool  // Set on thumb mouse_down, cleared on mouse_up
```

## Edge Cases

| Case | Behavior |
|------|----------|
| Content fits in viewport | No scrollbar rendered |
| Single line | Filly-height thumb, no interaction |
| Drag start outside thumb | Treat as track click (page) |
| Window resize | ListState adjusts viewport; next render updates scrollbar |
| File open with 5000+ lines | Unmeasured lines estimated; corrected on scroll |

## Files Changed

- `src/editor/mod.rs` — scrollbar rendering, mouse handling, state fields
- No new files needed
- No theme/config changes needed
