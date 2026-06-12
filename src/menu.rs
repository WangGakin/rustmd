use gpui::{
    Action, App, InteractiveElement, IntoElement, MouseButton, ParentElement,
    SharedString, Styled, actions, div, px,
};

use crate::editor::EditorTheme;
use crate::file_ops::{NewFile, OpenFile, Save};
use crate::key_mode::KeyMode;
use crate::window::NewWindow;

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
        "Mac"
    } else {
        "Win"
    };

    vec![
        ToolbarButton::new("\u{1F4C4}", NewFile),   // 📄
        ToolbarButton::new("\u{1F4C2}", OpenFile),  // 📂
        ToolbarButton::new("\u{1F4BE}", Save),      // 💾
        ToolbarButton::new("\u{1F532}", NewWindow), // 🔲
        ToolbarButton::new(format!("⌨ {}", mode_text), ToggleKeyMode),
    ]
}

pub fn toolbar(theme: &EditorTheme, cx: &mut App) -> impl IntoElement {
    let buttons = get_toolbar_buttons(cx);

    let mut button_elements = Vec::new();

    for (index, button) in buttons.into_iter().enumerate() {
        let action = button.action;
        let name = button.name.to_string();

        // Separator between Save group and NewWindow
        if index == 3 {
            button_elements.push(
                div()
                    .px(px(2.0))
                    .text_color(theme.comment)
                    .child("\u{2502}")  // │
                    .into_any_element(),
            );
        }

        let button_element = div()
            .id(("toolbar-btn", index))
            .px(px(8.0))
            .py(px(4.0))
            .text_color(theme.foreground)
            .cursor_pointer()
            .rounded(px(3.0))
            .hover(|s| s.bg(theme.selection))
            .child(name)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.dispatch_action(action.boxed_clone(), cx);
            });

        button_elements.push(button_element.into_any_element());
    }

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .children(button_elements)
}
