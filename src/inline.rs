//! Inline style extraction for markdown text.
//!
//! This module extracts styled regions (bold, italic, code, links, etc.)
//! from the inline parse trees.

use ropey::Rope;
use std::ops::Range;

use tree_sitter::Node;

use crate::parser::MarkdownTree;

/// Style attributes for inline text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strikethrough: bool,
    pub heading_level: u8,
}

impl TextStyle {
    pub fn bold() -> Self {
        Self {
            bold: true,
            ..Default::default()
        }
    }

    pub fn italic() -> Self {
        Self {
            italic: true,
            ..Default::default()
        }
    }

    pub fn code() -> Self {
        Self {
            code: true,
            ..Default::default()
        }
    }

    pub fn strikethrough() -> Self {
        Self {
            strikethrough: true,
            ..Default::default()
        }
    }

    pub fn heading(level: u8) -> Self {
        Self {
            heading_level: level,
            bold: true,
            ..Default::default()
        }
    }

    pub fn merge(&self, other: &TextStyle) -> Self {
        Self {
            bold: self.bold || other.bold,
            italic: self.italic || other.italic,
            code: self.code || other.code,
            strikethrough: self.strikethrough || other.strikethrough,
            heading_level: self.heading_level.max(other.heading_level),
        }
    }
}

/// A styled region of inline text with its delimiters.
#[derive(Debug, Clone, PartialEq)]
pub struct StyledRegion {
    /// The full range including delimiters (e.g., `**bold**` → 0..8)
    pub full_range: Range<usize>,
    /// The content range excluding delimiters (e.g., `**bold**` → 2..6)
    pub content_range: Range<usize>,
    pub style: TextStyle,
    pub link_url: Option<String>,
    pub is_image: bool,
    /// If Some, this is a checkbox. The bool indicates checked state.
    pub checkbox: Option<bool>,
    /// If Some, display this text instead of the buffer content.
    /// Used for GitHub URL shortening (e.g., show "owner/repo#123" instead of full URL).
    /// When set, the region is "atomic" - cursor/selection treat it as a single unit.
    pub display_text: Option<String>,
}

/// Extract all inline styles from a markdown tree.
/// Returns a flat Vec sorted by start byte position.
pub fn extract_all_inline_styles(tree: &MarkdownTree, rope: &Rope) -> Vec<StyledRegion> {
    let mut styles = Vec::new();

    let block_root = tree.block_tree().root_node();
    collect_from_block_tree(&block_root, tree, rope, &mut styles);

    styles.sort_by_key(|s| s.full_range.start);

    styles
}

/// Collect inline styles from the block tree by finding "inline" nodes.
fn collect_from_block_tree(
    node: &Node,
    tree: &MarkdownTree,
    rope: &Rope,
    styles: &mut Vec<StyledRegion>,
) {
    // Check if this node has an associated inline tree
    if (node.kind() == "inline" || node.kind() == "pipe_table_cell")
        && let Some(inline_tree) = tree.inline_tree(node)
    {
        collect_from_inline_tree(inline_tree.root_node(), rope, styles);
    }

    // Recurse into children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_from_block_tree(&child, tree, rope, styles);
    }
}

/// Collect styled regions from an inline tree.
fn collect_from_inline_tree(node: Node, rope: &Rope, styles: &mut Vec<StyledRegion>) {
    collect_from_inline_tree_inner(node, rope, styles, false);
}

/// Inner function that tracks whether we're inside a strikethrough.
fn collect_from_inline_tree_inner(
    node: Node,
    rope: &Rope,
    styles: &mut Vec<StyledRegion>,
    in_strikethrough: bool,
) {
    let mut child_in_strikethrough = in_strikethrough;

    match node.kind() {
        "emphasis" => {
            if let Some(region) = extract_emphasis_region(&node, TextStyle::italic()) {
                styles.push(region);
            }
        }
        "strong_emphasis" => {
            if let Some(region) = extract_emphasis_region(&node, TextStyle::bold()) {
                styles.push(region);
            }
        }
        "code_span" => {
            if let Some(region) = extract_code_span_region(&node) {
                styles.push(region);
            }
        }
        "strikethrough" => {
            // Skip nested strikethroughs - tree-sitter parses ~~text~~ as nested ~(~text~)~
            if !in_strikethrough {
                if let Some(region) = extract_emphasis_region(&node, TextStyle::strikethrough()) {
                    styles.push(region);
                }
                child_in_strikethrough = true;
            }
        }
        "inline_link" | "full_reference_link" | "collapsed_reference_link" | "shortcut_link" => {
            if let Some(region) = extract_link_region(&node, rope) {
                styles.push(region);
            }
        }
        "image" => {
            if let Some(region) = extract_image_region(&node, rope) {
                styles.push(region);
            }
        }
        _ => {}
    }

    // Recurse into children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_from_inline_tree_inner(child, rope, styles, child_in_strikethrough);
    }
}

fn extract_emphasis_region(node: &Node, style: TextStyle) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    let mut content_start = full_start;
    let mut content_end = full_end;

    // Find all delimiter boundaries recursively
    // This handles ~~text~~ which tree-sitter parses as nested ~(~text~)~
    fn collect_delimiters(node: &Node, delimiters: &mut Vec<(usize, usize)>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let kind = child.kind();
            if kind == "emphasis_delimiter" || kind.ends_with("_delimiter") {
                delimiters.push((child.start_byte(), child.end_byte()));
            }
            // Recurse into nested emphasis/strikethrough of the same type
            if kind == node.kind() {
                collect_delimiters(&child, delimiters);
            }
        }
    }

    let mut delimiters: Vec<(usize, usize)> = Vec::new();
    collect_delimiters(node, &mut delimiters);

    // Opening delimiters from start - keep consuming adjacent delimiters
    delimiters.sort_by_key(|(start, _)| *start);
    for &(start, end) in &delimiters {
        if start == content_start {
            content_start = end;
        }
    }

    // Closing delimiters from end - keep consuming adjacent delimiters
    for &(start, end) in delimiters.iter().rev() {
        if end == content_end {
            content_end = start;
        }
    }

    Some(StyledRegion {
        full_range: full_start..full_end,
        content_range: content_start..content_end,
        style,
        link_url: None,
        is_image: false,
        checkbox: None,
        display_text: None,
    })
}

fn extract_code_span_region(node: &Node) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    let mut content_start = full_start;
    let mut content_end = full_end;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "code_span_delimiter" {
            if child.start_byte() == full_start {
                content_start = child.end_byte();
            } else if child.end_byte() == full_end {
                content_end = child.start_byte();
            }
        }
    }

    Some(StyledRegion {
        full_range: full_start..full_end,
        content_range: content_start..content_end,
        style: TextStyle::code(),
        link_url: None,
        is_image: false,
        checkbox: None,
        display_text: None,
    })
}

fn extract_link_region(node: &Node, rope: &Rope) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    // Skip task list checkbox patterns like [ ] or [x] or [X]
    // These get misdetected as shortcut_links when tree-sitter doesn't
    // recognize the task list (e.g., when there's no content after the checkbox)
    if node.kind() == "shortcut_link" {
        let start = rope.byte_to_char(full_start);
        let end = rope.byte_to_char(full_end);
        let text = rope.slice(start..end).to_string();
        if text == "[ ]" || text == "[x]" || text == "[X]" {
            return None;
        }
    }

    let mut content_start = full_start;
    let mut content_end = full_end;
    let mut url: Option<String> = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "link_text" => {
                content_start = child.start_byte();
                content_end = child.end_byte();
            }
            "link_destination" => {
                let start = rope.byte_to_char(child.start_byte());
                let end = rope.byte_to_char(child.end_byte());
                url = Some(rope.slice(start..end).to_string());
            }
            _ => {}
        }
    }

    // Fallback for reference-style links without explicit link_text
    if url.is_none() {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "[" {
                content_start = child.end_byte();
            } else if child.kind() == "]" {
                content_end = child.start_byte();
            }
        }
    }

    Some(StyledRegion {
        full_range: full_start..full_end,
        content_range: content_start..content_end,
        style: TextStyle::default(),
        link_url: url,
        is_image: false,
        checkbox: None,
        display_text: None,
    })
}

fn extract_image_region(node: &Node, rope: &Rope) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    let mut alt_start = full_start;
    let mut alt_end = full_end;
    let mut url: Option<String> = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "image_description" => {
                alt_start = child.start_byte();
                alt_end = child.end_byte();
            }
            "link_destination" => {
                let start = rope.byte_to_char(child.start_byte());
                let end = rope.byte_to_char(child.end_byte());
                url = Some(rope.slice(start..end).to_string());
            }
            _ => {}
        }
    }

    let url = url?;

    Some(StyledRegion {
        full_range: full_start..full_end,
        content_range: alt_start..alt_end,
        style: TextStyle::default(),
        link_url: Some(url),
        is_image: true,
        checkbox: None,
        display_text: None,
    })
}

/// Get inline styles that overlap with a byte range.
/// Uses binary search for efficient lookup.
pub fn styles_in_range<'a>(
    styles: &'a [StyledRegion],
    range: &Range<usize>,
) -> Vec<&'a StyledRegion> {
    if styles.is_empty() {
        return Vec::new();
    }

    // Binary search to find first style that might overlap
    let start_idx = styles
        .binary_search_by_key(&range.start, |s| s.full_range.start)
        .unwrap_or_else(|idx| idx.saturating_sub(1));

    let mut result = Vec::new();
    for style in &styles[start_idx..] {
        // Stop if we're past the range
        if style.full_range.start >= range.end {
            break;
        }
        // Include if overlapping
        if style.full_range.end > range.start {
            result.push(style);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;

    fn get_styles(text: &str) -> Vec<StyledRegion> {
        let buf: Buffer = text.parse().unwrap();
        extract_all_inline_styles(buf.tree().unwrap(), buf.rope())
    }

    #[test]
    fn test_bold() {
        let styles = get_styles("**bold** text\n");
        assert_eq!(styles.len(), 1);
        assert!(styles[0].style.bold);
        assert_eq!(styles[0].full_range, 0..8);
        assert_eq!(styles[0].content_range, 2..6);
    }

    #[test]
    fn test_italic() {
        let styles = get_styles("*italic* text\n");
        assert_eq!(styles.len(), 1);
        assert!(styles[0].style.italic);
        assert_eq!(styles[0].full_range, 0..8);
        assert_eq!(styles[0].content_range, 1..7);
    }

    #[test]
    fn test_code() {
        let styles = get_styles("`code` text\n");
        assert_eq!(styles.len(), 1);
        assert!(styles[0].style.code);
        assert_eq!(styles[0].full_range, 0..6);
        assert_eq!(styles[0].content_range, 1..5);
    }

    #[test]
    fn test_link() {
        let styles = get_styles("[text](http://example.com)\n");
        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].link_url, Some("http://example.com".to_string()));
        assert_eq!(styles[0].full_range, 0..26);
        // content_range should be the link text "text"
        assert_eq!(styles[0].content_range, 1..5);
    }

    #[test]
    fn test_nested_bold_italic() {
        let styles = get_styles("***bold italic***\n");
        // Should have both bold and italic regions
        assert!(!styles.is_empty());
    }

    #[test]
    fn test_multiple_lines() {
        let styles = get_styles("**bold**\n*italic*\n`code`\n");
        assert_eq!(styles.len(), 3);
        // Should be sorted by position
        assert!(styles[0].style.bold);
        assert!(styles[1].style.italic);
        assert!(styles[2].style.code);
    }

    #[test]
    fn test_styles_in_range() {
        let styles = get_styles("**bold**\n*italic*\n`code`\n");

        // Line 1: bytes 0-8
        let line1_styles = styles_in_range(&styles, &(0..8));
        assert_eq!(line1_styles.len(), 1);
        assert!(line1_styles[0].style.bold);

        // Line 2: bytes 9-17
        let line2_styles = styles_in_range(&styles, &(9..17));
        assert_eq!(line2_styles.len(), 1);
        assert!(line2_styles[0].style.italic);
    }

    #[test]
    fn test_blockquote_inline() {
        let styles = get_styles("> **bold** in quote\n");
        assert_eq!(styles.len(), 1);
        assert!(styles[0].style.bold);
    }

    #[test]
    fn test_list_inline() {
        let styles = get_styles("- **bold** in list\n- *italic* too\n");
        assert_eq!(styles.len(), 2);
        assert!(styles[0].style.bold);
        assert!(styles[1].style.italic);
    }

    #[test]
    fn test_strikethrough() {
        let styles = get_styles("~~hey~~\n");
        // Tree-sitter parses ~~hey~~ as nested strikethrough ~(~hey~)~
        // We skip the inner one and collect all delimiters recursively
        assert_eq!(styles.len(), 1);
        assert!(styles[0].style.strikethrough);
        // full_range is the entire ~~hey~~ (0..7)
        // content_range excludes all delimiters (2..5 for just "hey")
        assert_eq!(styles[0].full_range, 0..7);
        assert_eq!(styles[0].content_range, 2..5);
    }

}
