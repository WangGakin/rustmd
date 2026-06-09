use std::ops::Range;

use gpui::Action;

/// An action that can be executed on the editor programmatically.
///
/// Use with [`Editor::execute`](super::Editor::execute) to control the editor
/// from code, such as in scripted demos.
#[derive(Clone, Debug, PartialEq)]
pub enum EditorAction {
    /// Insert a character at the cursor.
    Type(char),
    /// Insert a raw newline.
    Enter,
    /// Continue container: adds markers from current line.
    ShiftEnter,
    /// Indented continuation: creates nested paragraph.
    ShiftAltEnter,
    /// Tab: cycles forward through nesting states based on context.
    Tab,
    /// Shift-Tab: cycles backward through nesting states.
    ShiftTab,
    /// Delete the character before the cursor (markers are atomic).
    Backspace,
    /// Move the cursor in a direction.
    Move(Direction),
    /// Click at a buffer offset.
    Click {
        offset: usize,
        shift: bool,
        click_count: usize,
    },
    /// Drag to extend selection to a buffer offset.
    Drag { offset: usize },
    /// Toggle a checkbox on a line.
    ToggleCheckbox { line_number: usize },
    /// Update hover state.
    UpdateHover {
        over_checkbox: bool,
        over_link: bool,
        /// Byte range of hovered GitHub ref (if any).
        hovered_github_ref_range: Option<Range<usize>>,
        /// Screen position of the hovered GitHub ref (for popup positioning).
        hovered_ref_position: Option<gpui::Point<gpui::Pixels>>,
    },
    /// Open a link URL.
    OpenLink { url: String },
}

/// Cursor movement direction.
#[derive(Clone, Debug, PartialEq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Wrapper to dispatch EditorAction via GPUI's action system.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct DispatchEditorAction(pub EditorAction);
