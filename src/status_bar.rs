use gpui::{App, Global, ReadGlobal, div, prelude::*, px, rems};

use crate::config::Config;
use crate::editor::EditorTheme;
use crate::marker::MarkerKind;

/// Status bar information updated by the editor on each render.
#[derive(Clone, Default, PartialEq)]
pub struct StatusBarInfo {
    /// Context markers for the current line
    pub context_markers: Vec<MarkerKind>,
    /// Current heading level (1-6), if cursor is under a heading
    pub heading_level: Option<u8>,
    /// Cursor line (1-indexed)
    pub cursor_line: usize,
    /// Cursor column (1-indexed)
    pub cursor_col: usize,
    /// Total number of lines in the document
    pub total_lines: usize,
    /// First visible line (0-indexed), for scroll percentage
    pub first_visible_line: usize,
    /// Last visible line (0-indexed), for scroll percentage
    pub last_visible_line: usize,
}

impl Global for StatusBarInfo {}

/// Render the status bar at the bottom of the editor.
pub fn status_bar(cx: &App) -> impl IntoElement {
    let info = StatusBarInfo::global(cx);
    let theme = EditorTheme::global(cx);
    let config = Config::global(cx);

    // Scroll indicator: Top/Bot/All or percentage
    let scroll_str = if info.total_lines <= 1
        || (info.first_visible_line == 0 && info.last_visible_line >= info.total_lines - 1)
    {
        "All".to_string()
    } else if info.first_visible_line == 0 {
        "Top".to_string()
    } else if info.last_visible_line >= info.total_lines - 1 {
        "Bot".to_string()
    } else {
        let pct = ((info.last_visible_line + 1) * 100) / info.total_lines;
        format!("{}%", pct)
    };

    // Position: Ln 42, Col 15
    let position_str = format!("Ln {}, Col {}", info.cursor_line, info.cursor_col);

    // Color palette for nesting depth (cycles when exhausted)
    let depth_colors = [
        theme.cyan,
        theme.purple,
        theme.green,
        theme.orange,
        theme.pink,
        theme.yellow,
    ];

    // Build display info using shared logic
    let display_items = build_context_display(&info.context_markers);

    // Convert to colored UI elements
    let mut marker_elements: Vec<gpui::AnyElement> = Vec::new();
    for (i, (display_str, depth)) in display_items.iter().enumerate() {
        // Add space separator (nested checkboxes like " ]" already include their space)
        let needs_space = !display_str.starts_with(' ') && !display_str.starts_with('x');
        if i > 0 && needs_space {
            marker_elements.push(div().child(" ").into_any_element());
        }

        let color = depth_colors[*depth % depth_colors.len()];
        marker_elements.push(
            div()
                .text_color(color)
                .child(display_str.clone())
                .into_any_element(),
        );
    }

    div()
        .w_full()
        .py(rems(0.25))
        .px(rems(2.0))
        .bg(theme.background)
        .border_color(theme.selection)
        .border_t_1()
        .font_family(config.code_font.clone())
        .text_color(theme.comment)
        .child(
            div()
                .w_full()
                .max_w(px(800.0))
                .mx_auto()
                .flex()
                .flex_row()
                .justify_between()
                .child(
                    // Left: context markers with depth colors
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .flex()
                        .flex_row()
                        .children(marker_elements),
                )
                .child(
                    // Right: heading, position, lines, scroll
                    div()
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .flex()
                        .flex_row()
                        .items_center()
                        .children(info.heading_level.map(|level| {
                            div()
                                .text_color(theme.cyan)
                                .mr(rems(1.0))
                                .child(format!("H{}", level))
                        }))
                        .child(position_str)
                        .child(div().mx(rems(0.5)).text_color(theme.selection).child("·"))
                        .child(format!("{} lines", info.total_lines))
                        .child(div().mx(rems(0.5)).text_color(theme.selection).child("·"))
                        .child(div().text_color(theme.purple).child(scroll_str)),
                ),
        )
}

/// Build the display string for context markers, handling nested checkbox compaction.
/// Returns a vector of (display_string, depth) tuples.
pub fn build_context_display(markers: &[MarkerKind]) -> Vec<(String, usize)> {
    let mut result = Vec::new();
    let mut depth = 0;
    let mut prev_was_checkbox = false;

    for (i, marker) in markers.iter().enumerate() {
        // Skip list marker if previous was checkbox (they're grouped together)
        if marker.is_list_item() && prev_was_checkbox {
            if i > 0 {
                depth += 1;
            }
            continue;
        }

        // Increment depth before block-level markers (except first)
        if i > 0 && marker.is_block_level() {
            depth += 1;
        }

        // Determine display string (nested checkboxes use compact form)
        let display_str = match marker {
            MarkerKind::Checkbox { checked: false } if prev_was_checkbox => " ]".to_string(),
            MarkerKind::Checkbox { checked: true } if prev_was_checkbox => "x]".to_string(),
            _ => marker.status_bar_str(),
        };

        result.push((display_str, depth));
        prev_was_checkbox = marker.is_checkbox();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marker::{OrderedMarker, UnorderedMarker};

    fn unordered_list(marker: UnorderedMarker) -> MarkerKind {
        MarkerKind::ListItem {
            ordered: false,
            unordered_marker: Some(marker),
            ordered_marker: None,
            number: None,
        }
    }

    fn ordered_list(number: u32, style: OrderedMarker) -> MarkerKind {
        MarkerKind::ListItem {
            ordered: true,
            unordered_marker: None,
            ordered_marker: Some(style),
            number: Some(number),
        }
    }

    #[test]
    fn status_bar_str_blockquote() {
        assert_eq!(MarkerKind::BlockQuote.status_bar_str(), ">");
    }

    #[test]
    fn status_bar_str_unordered_list_minus() {
        assert_eq!(unordered_list(UnorderedMarker::Minus).status_bar_str(), "-");
    }

    #[test]
    fn status_bar_str_unordered_list_star() {
        assert_eq!(unordered_list(UnorderedMarker::Star).status_bar_str(), "*");
    }

    #[test]
    fn status_bar_str_unordered_list_plus() {
        assert_eq!(unordered_list(UnorderedMarker::Plus).status_bar_str(), "+");
    }

    #[test]
    fn status_bar_str_ordered_list_dot() {
        assert_eq!(ordered_list(1, OrderedMarker::Dot).status_bar_str(), "1.");
        assert_eq!(ordered_list(42, OrderedMarker::Dot).status_bar_str(), "42.");
    }

    #[test]
    fn status_bar_str_ordered_list_parenthesis() {
        assert_eq!(
            ordered_list(1, OrderedMarker::Parenthesis).status_bar_str(),
            "1)"
        );
        assert_eq!(
            ordered_list(10, OrderedMarker::Parenthesis).status_bar_str(),
            "10)"
        );
    }

    #[test]
    fn status_bar_str_checkbox_unchecked() {
        assert_eq!(
            MarkerKind::Checkbox { checked: false }.status_bar_str(),
            "[ ]"
        );
    }

    #[test]
    fn status_bar_str_checkbox_checked() {
        assert_eq!(
            MarkerKind::Checkbox { checked: true }.status_bar_str(),
            "[x]"
        );
    }

    #[test]
    fn status_bar_str_code_block_no_language() {
        assert_eq!(
            MarkerKind::CodeBlockFence {
                language: None,
                is_opening: true
            }
            .status_bar_str(),
            "```"
        );
    }

    #[test]
    fn status_bar_str_code_block_with_language() {
        assert_eq!(
            MarkerKind::CodeBlockFence {
                language: Some("rust".to_string()),
                is_opening: true
            }
            .status_bar_str(),
            "```rust"
        );
    }

    #[test]
    fn display_simple_list() {
        let markers = vec![unordered_list(UnorderedMarker::Minus)];
        let display = build_context_display(&markers);
        assert_eq!(display, vec![("-".to_string(), 0)]);
    }

    #[test]
    fn display_nested_lists() {
        let markers = vec![
            unordered_list(UnorderedMarker::Minus),
            unordered_list(UnorderedMarker::Minus),
        ];
        let display = build_context_display(&markers);
        assert_eq!(display, vec![("-".to_string(), 0), ("-".to_string(), 1)]);
    }

    #[test]
    fn display_list_with_checkbox() {
        // - [x] displays as "- [x]" at same depth
        let markers = vec![
            unordered_list(UnorderedMarker::Minus),
            MarkerKind::Checkbox { checked: true },
        ];
        let display = build_context_display(&markers);
        assert_eq!(display, vec![("-".to_string(), 0), ("[x]".to_string(), 0)]);
    }

    #[test]
    fn display_nested_checkboxes_compact() {
        // - [x] - [ ] displays as "- [x] ]" (nested checkbox compacted)
        let markers = vec![
            unordered_list(UnorderedMarker::Minus),
            MarkerKind::Checkbox { checked: true },
            unordered_list(UnorderedMarker::Minus),
            MarkerKind::Checkbox { checked: false },
        ];
        let display = build_context_display(&markers);
        // After [x], the next - is skipped, depth increments, then [ ] shows as " ]"
        assert_eq!(
            display,
            vec![
                ("-".to_string(), 0),
                ("[x]".to_string(), 0),
                (" ]".to_string(), 1)
            ]
        );
    }

    #[test]
    fn display_nested_checked_checkbox_compact() {
        // - [ ] - [x] displays as "- [ ]x]"
        let markers = vec![
            unordered_list(UnorderedMarker::Minus),
            MarkerKind::Checkbox { checked: false },
            unordered_list(UnorderedMarker::Minus),
            MarkerKind::Checkbox { checked: true },
        ];
        let display = build_context_display(&markers);
        assert_eq!(
            display,
            vec![
                ("-".to_string(), 0),
                ("[ ]".to_string(), 0),
                ("x]".to_string(), 1)
            ]
        );
    }

    #[test]
    fn display_blockquote_list() {
        let markers = vec![
            MarkerKind::BlockQuote,
            unordered_list(UnorderedMarker::Minus),
        ];
        let display = build_context_display(&markers);
        assert_eq!(display, vec![(">".to_string(), 0), ("-".to_string(), 1)]);
    }

    #[test]
    fn display_ordered_list() {
        let markers = vec![ordered_list(3, OrderedMarker::Dot)];
        let display = build_context_display(&markers);
        assert_eq!(display, vec![("3.".to_string(), 0)]);
    }

    #[test]
    fn display_code_block() {
        let markers = vec![MarkerKind::CodeBlockFence {
            language: Some("python".to_string()),
            is_opening: true,
        }];
        let display = build_context_display(&markers);
        assert_eq!(display, vec![("```python".to_string(), 0)]);
    }

    #[test]
    fn display_deeply_nested() {
        // > - [x] - [ ] - [x]
        let markers = vec![
            MarkerKind::BlockQuote,
            unordered_list(UnorderedMarker::Minus),
            MarkerKind::Checkbox { checked: true },
            unordered_list(UnorderedMarker::Minus),
            MarkerKind::Checkbox { checked: false },
            unordered_list(UnorderedMarker::Minus),
            MarkerKind::Checkbox { checked: true },
        ];
        let display = build_context_display(&markers);
        // > at depth 0, - [x] at depth 1, ] at depth 2, x] at depth 3
        assert_eq!(
            display,
            vec![
                (">".to_string(), 0),
                ("-".to_string(), 1),
                ("[x]".to_string(), 1),
                (" ]".to_string(), 2),
                ("x]".to_string(), 3),
            ]
        );
    }

    #[test]
    fn depth_cycles_after_six() {
        // 7 nested lists should cycle back to depth 0 for color
        let markers = vec![
            unordered_list(UnorderedMarker::Minus),
            unordered_list(UnorderedMarker::Minus),
            unordered_list(UnorderedMarker::Minus),
            unordered_list(UnorderedMarker::Minus),
            unordered_list(UnorderedMarker::Minus),
            unordered_list(UnorderedMarker::Minus),
            unordered_list(UnorderedMarker::Minus),
        ];
        let display = build_context_display(&markers);
        assert_eq!(display.len(), 7);
        // Depths: 0, 1, 2, 3, 4, 5, 6
        assert_eq!(display[6].1, 6);
        // Color cycling happens in status_bar() with depth % 6
    }
}
