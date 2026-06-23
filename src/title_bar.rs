use gpui::{Action, App, MouseButton, div, prelude::*, px, rems};
#[cfg(windows)]
use gpui::{ElementId, Fill};
#[cfg(windows)]
use raw_window_handle::RawWindowHandle;
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, HTCAPTION, WM_NCLBUTTONDOWN};

use crate::editor::EditorTheme;
use crate::menu;
#[cfg(windows)]
use crate::window::{CloseWindow, MinimizeWindow, ZoomWindow};
use crate::file_explorer::ToggleFileExplorer;

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct ToggleRecentFiles;

pub struct FileInfo {
    pub path: Option<std::path::PathBuf>,
    pub dirty: bool,
    pub recent_files: Vec<String>,
}

#[cfg(windows)]
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

    let title_bar = div()
        .id("title-bar")
        .w_full()
        .py(rems(0.5))
        .px(rems(1.0));

    #[cfg(target_os = "macos")]
    let title_bar = title_bar.pl(px(74.0));

    title_bar
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
                            #[cfg(not(windows))]
                            window.start_window_move();
                            #[cfg(windows)]
                            if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window)
                                && let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                                    unsafe {
                                        let hwnd = HWND(win32_handle.hwnd.get() as _);
                                        let _ = ReleaseCapture();
                                        let _ = PostMessageW(Some(hwnd), WM_NCLBUTTONDOWN, WPARAM(HTCAPTION as _), LPARAM(0));
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
                                .flex()
                                .items_center()
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
                .child({
                    let action = ToggleFileExplorer.boxed_clone();
                    div()
                        .id("file-explorer-btn")
                        .px(px(6.0))
                        .py(px(3.0))
                        .text_color(theme.foreground)
                        .rounded(px(3.0))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.selection))
                        .child("\u{1F4C1}")
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            window.dispatch_action(action.boxed_clone(), cx);
                        })
                })
                .gap(rems(0.5))
                .child({
                    let action = ToggleRecentFiles.boxed_clone();
                    div()
                        .id("recent-files-btn")
                        .px(px(6.0))
                        .py(px(3.0))
                        .rounded(px(3.0))
                        .hover(|s| s.bg(theme.selection))
                        .when(has_recent, |this| {
                            this.cursor_pointer()
                        })
                        .child("\u{1F552}")
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            window.dispatch_action(action.boxed_clone(), cx);
                        })
                })
                .child({
                    #[cfg(windows)]
                    { traffic_light(
                        "minimize-button",
                        theme.orange,
                        MinimizeWindow,
                    ) }
                    #[cfg(not(windows))]
                    { div() }
                })
                .child({
                    #[cfg(windows)]
                    { traffic_light("maximize-button", theme.green, ZoomWindow) }
                    #[cfg(not(windows))]
                    { div() }
                })
                .child({
                    #[cfg(windows)]
                    { traffic_light("quit-button", theme.red, CloseWindow) }
                    #[cfg(not(windows))]
                    { div() }
                }),
        )
}
