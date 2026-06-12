//! A Typora-style markdown editor component for GPUI.
//!
//! Writ provides an embeddable markdown editor with live inline rendering—markers
//! like `**`, `#`, and `-` are hidden when the cursor is elsewhere, showing only
//! the styled result.
//!
//! # Features
//!
//! - **Live inline rendering**: Markdown syntax is hidden when not editing
//! - **Syntax highlighting**: Code blocks with tree-sitter based highlighting
//! - **Smart continuation**: Shift+Enter continues lists, blockquotes, etc.
//! - **Naked URL detection**: Plain URLs are automatically detected
//!
//! # Quick Start
//!
//! ```ignore
//! use writ::{Editor, EditorConfig};
//!
//! // Create with default config
//! let editor = cx.new(|cx| Editor::new("# Hello", cx));
//!
//! // Or with custom config
//! let config = EditorConfig::default();
//! let editor = cx.new(|cx| Editor::with_config("# Hello", config, cx));
//! ```
//!
//! # Streaming
//!
//! For AI chat or other streaming use cases:
//!
//! ```ignore
//! editor.update(cx, |e, cx| e.begin_streaming(cx));
//! for token in stream {
//!     editor.update(cx, |e, cx| e.append(&token, cx));
//! }
//! editor.update(cx, |e, cx| e.end_streaming(cx));
//! ```

pub use editor::{Direction, Editor, EditorAction, EditorConfig, EditorTheme};

pub mod buffer;
pub mod config;
pub mod cursor;
pub mod demo;
pub mod editor;
pub mod file_ops;
pub mod highlight;
pub mod inline;
pub mod key_mode;
pub mod line;
pub mod marker;
pub mod menu;
pub mod parser;
pub mod paste;
pub mod status_bar;
pub mod title_bar;
pub mod user_config;
pub mod window;
