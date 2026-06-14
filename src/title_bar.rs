use gpui::{Action, App, ElementId, Fill, MouseButton, div, prelude::*, px, rems};
use raw_window_handle::RawWindowHandle;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, HTCAPTION, WM_NCLBUTTONDOWN};

use crate::editor::EditorTheme;
use crate::menu;
use crate::window::{CloseWindow, MinimizeWindow, ZoomWindow};

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct ToggleRecentFiles;

pub struct FileInfo {
    pub path: Option<std::path::PathBuf>,
    pub dirty: bool,
    pub recent_files: Vec<String>,
}

fn traffic_light(
    id: impl Into<ElementId>,
    bg: impl Into<Fill>,
    action: impl Action,
) -> impl IntoElement {
    div()
        .id(id)
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.opacity(0.7))
        .child(div().w(rems(1.0)).h(rems(1.0)).rounded_full().bg(bg))
        .on_click({
            let action = action.boxed_clone();
            move |_, window, cx| {
                window.dispatch_action(action.boxed_clone(), cx);
            }
        })
}

pub fn title_bar(theme: &EditorTheme, file_info: &FileInfo, cx: &mut App) -> impl IntoElement {
    let file_name = match &file_info.path {
        Some(path) => path
            .file_name()
            .map(|n| n.display().to_string())
            .unwrap_or_else(|| "untitled".to_string()),
        None => "untitled".to_string(),
    };
    let title = if file_info.dirty {
        format!("* {}", file_name)
    } else {
        file_name
    };

    let has_recent = !file_info.recent_files.is_empty();

    div()
        .id("title-bar")
        .w_full()
        .py(rems(0.5))
        .px(rems(1.0))
        .border_color(theme.selection)
        .border_b_1()
        .flex()
        .flex_row()
        .justify_between()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(rems(1.0))
                .child(menu::toolbar(theme, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .relative()
                        .on_mouse_down(MouseButton::Left, |_e, window, _cx| {
                            if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) {
                                if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                                    unsafe {
                                        let hwnd = HWND(win32_handle.hwnd.get() as _);
                                        let _ = ReleaseCapture();
                                        let _ = SendMessageW(hwnd, WM_NCLBUTTONDOWN, Some(WPARAM(HTCAPTION as _)), Some(LPARAM(0)));
                                    }
                                }
                            }
                        })
                        .child(
                            div()
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .invisible()
                                .child(title.clone()),
                        )
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top_0()
                                .bottom_0()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(title),
                        ),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(rems(0.5))
                .child({
                    let action = ToggleRecentFiles.boxed_clone();
                    div()
                        .id("recent-files-btn")
                        .px(px(6.0))
                        .py(px(3.0))
                        .text_color(if has_recent { theme.foreground } else { theme.comment })
                        .rounded(px(3.0))
                        .when(has_recent, |this| {
                            this.cursor_pointer().hover(|s| s.bg(theme.selection))
                        })
                        .child("\u{1F552}")
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            window.dispatch_action(action.boxed_clone(), cx);
                        })
                })
                .child(traffic_light(
                    "minimize-button",
                    theme.orange,
                    MinimizeWindow,
                ))
                .child(traffic_light("maximize-button", theme.green, ZoomWindow))
                .child(traffic_light("quit-button", theme.red, CloseWindow)),
        )
}
