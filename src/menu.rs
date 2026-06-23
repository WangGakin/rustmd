use gpui::{
    Action, App, InteractiveElement, IntoElement, MouseButton, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, actions, div, px,
};

use crate::editor::{EditorTheme, ToggleFind};
use crate::file_ops::{NewFile, OpenFile, Save};
use crate::key_mode::KeyMode;
use crate::tooltip::Tooltip;
use crate::window::NewWindow;

actions!(menu, [ToggleKeyMode, ToggleAbout]);

pub struct ToolbarButton {
    pub name: SharedString,
    pub action: Box<dyn Action>,
    pub description: &'static str,
}

impl ToolbarButton {
    pub fn new(
        name: impl Into<SharedString>,
        description: &'static str,
        action: impl Action,
    ) -> Self {
        Self {
            name: name.into(),
            description,
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
        ToolbarButton::new("\u{1F980}", "About", ToggleAbout),
        ToolbarButton::new("\u{1F4C4}", "New file", NewFile),
        ToolbarButton::new("\u{1F4C2}", "Open file", OpenFile),
        ToolbarButton::new("\u{1F4BE}", "Save", Save),
        ToolbarButton::new("\u{1F50D}", "Find & Replace", ToggleFind),
        ToolbarButton::new("\u{1F532}", "New window", NewWindow),
        ToolbarButton::new(
            format!("⌨ {}", mode_text),
            "Keyboard mode",
            ToggleKeyMode,
        ),
    ]
}

fn tooltip_text(button: &ToolbarButton) -> String {
    match button.description {
        "About" => "About rustmd".into(),
        "New file" => "New file (Ctrl+Alt+N)".into(),
        "Open file" => "Open file (Ctrl+O)".into(),
        "Save" => "Save (Ctrl+S)".into(),
        "Find & Replace" => "Find and replace".into(),
        "New window" => "New window (Ctrl+Shift+N)".into(),
        "Keyboard mode" => {
            let mode = button.name.to_string();
            format!("Keyboard mode: {}", mode.trim_start_matches("⌨ "))
        }
        desc => desc.into(),
    }
}

pub fn toolbar(theme: &EditorTheme, cx: &mut App) -> impl IntoElement {
    let buttons = get_toolbar_buttons(cx);

    let mut button_elements = Vec::new();

    for (index, button) in buttons.into_iter().enumerate() {
        let tooltip = tooltip_text(&button);
        let action = button.action;
        let name = button.name.to_string();

        // Separator between Save group and NewWindow
        if index == 5 {
            button_elements.push(
                div()
                    .px(px(2.0))
                    .text_color(theme.comment)
                    .child("\u{2502}")
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
                Tooltip::hide(cx);
                window.dispatch_action(action.boxed_clone(), cx);
            })
            .on_hover({
                let tip = tooltip.clone();
                move |hovered, window, cx| {
                    if *hovered {
                        Tooltip::show(&tip, Some(window.mouse_position()), cx);
                    } else {
                        Tooltip::hide(cx);
                    }
                }
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
