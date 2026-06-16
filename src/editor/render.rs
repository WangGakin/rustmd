use std::collections::HashMap;
use std::ops::Range;
use std::cell::RefCell;
use std::rc::Rc;

use log::warn;

use gpui::{
    AnyElement, App, Context, Corner, CursorStyle, DragMoveEvent, FocusHandle, Focusable,
    Hsla, IntoElement, MouseButton, Pixels, Render,
    Rgba, Window, anchored, div, font, list, point, prelude::*, px,
};

use crate::buffer::RenderSnapshot;
use crate::cursor::Selection;
use crate::inline::StyledRegion;
use crate::key_mode::KeyMode;
use crate::line::{Line, LineParams, LineTheme, CursorScreenPosition};
use crate::status_bar::StatusBarInfo;

use super::*;

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

        // Detect naked URLs in visible lines — only when content changed
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

                    // Case A: line is visible and measured → center immediately
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

                    // Case B: line not measured — reveal it, then center after layout
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

impl Editor {
    /// Get a render snapshot of the current buffer state.
    /// Useful for capturing state before agent edits.
    pub fn render_snapshot(&mut self) -> RenderSnapshot {
        self.state.buffer.render_snapshot()
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
                            if first_delta > 3.0 {
                                editor.scrollbar_pending_page_turn = false;
                                // Anchor the drag start to current position so the
                                // first real scroll delta is small, not the full
                                // accumulated movement from the original click.
                                editor.scrollbar_drag_start_y = Some(mouse_y);
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
