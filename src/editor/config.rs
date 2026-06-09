use std::path::PathBuf;

use gpui::{Pixels, Rems, px};

use super::theme::{DEFAULT_CODE_FONT, DEFAULT_TEXT_FONT, EditorTheme};

/// Configuration for an [`Editor`](super::Editor) instance.
///
/// # Example
///
/// ```ignore
/// let config = EditorConfig {
///     theme: EditorTheme::dracula(),
///     text_font: "Inter".to_string(),
///     code_font: "JetBrains Mono".to_string(),
///     ..Default::default()
/// };
/// let editor = cx.new(|cx| Editor::with_config("# Hello", config, cx));
/// ```
#[derive(Clone)]
pub struct EditorConfig {
    /// Color theme for the editor.
    pub theme: EditorTheme,
    /// Font family for regular text.
    pub text_font: String,
    /// Font family for code blocks and inline code.
    pub code_font: String,
    /// Base path for resolving relative image URLs.
    pub base_path: Option<PathBuf>,
    /// Horizontal padding (left and right).
    pub padding_x: Rems,
    /// Top padding (before first line).
    pub padding_top: Rems,
    /// Bottom padding (after last line).
    pub padding_bottom: Rems,
    /// Line height for text lines.
    pub line_height: Rems,
    /// Maximum width for line content. None means fill container.
    pub max_line_width: Option<Pixels>,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            theme: EditorTheme::default(),
            text_font: DEFAULT_TEXT_FONT.to_string(),
            code_font: DEFAULT_CODE_FONT.to_string(),
            base_path: None,
            padding_x: Rems(0.0),
            padding_top: Rems(1.6),
            padding_bottom: Rems(4.8),
            line_height: Rems(1.6),
            max_line_width: Some(px(800.0)),
        }
    }
}
