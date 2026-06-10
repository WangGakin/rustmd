use gpui::{
    Action, App, InteractiveElement, IntoElement, MouseButton, ParentElement,
    SharedString, Styled, actions, div, px,
};

use crate::editor::EditorTheme;
use crate::file_ops::{NewFile, OpenFile, Save, SaveAs};
use crate::key_mode::KeyMode;

actions!(menu, [ToggleKeyMode]);

pub struct ToolbarButton {
    pub name: SharedString,
    pub action: Box<dyn Action>,
}

impl ToolbarButton {
    pub fn new(name: impl Into<SharedString>, action: impl Action) -> Self {
        Self {
            name: name.into(),
            action: Box::new(action),
        }
    }
}

pub fn get_toolbar_buttons(cx: &App) -> Vec<ToolbarButton> {
    let mode_text = if KeyMode::is_mac(cx) {
        "Mac Mode"
    } else {
        "Win Mode"
    };

    vec![
        ToolbarButton::new("New", NewFile),
        ToolbarButton::new("Open", OpenFile),
        ToolbarButton::new("Save", Save),
        ToolbarButton::new("Save As", SaveAs),
        ToolbarButton::new(mode_text, ToggleKeyMode),
    ]
}

pub fn toolbar(theme: &EditorTheme, cx: &mut App) -> impl IntoElement {
    let buttons = get_toolbar_buttons(cx);

    let mut button_elements = Vec::new();

    for (index, button) in buttons.into_iter().enumerate() {
        let action = button.action;
        let name = button.name.to_string();

        let button_element = div()
            .id(("toolbar-btn", index))
            .px(px(10.0))
            .py(px(4.0))
            .text_color(theme.foreground)
            .cursor_pointer()
            .rounded(px(3.0))
            .hover(|s| s.bg(theme.selection))
            .child(name)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.dispatch_action(action.boxed_clone(), cx);
            });

        button_elements.push(button_element);
    }

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .children(button_elements)
}
