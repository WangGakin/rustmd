use clap::Parser;
use gpui::*;
use raw_window_handle::RawWindowHandle;
use rustmd::config::Config;
use rustmd::editor::ime::EditorImeElement;
use rustmd::editor::{Editor, EditorTheme};
use rustmd::file_ops::{NewFile, OpenFile, Save, SaveAs};
use rustmd::key_mode::KeyMode;
use rustmd::line::CursorScreenPosition;
use rustmd::status_bar::StatusBarInfo;
use rustmd::title_bar::FileInfo;
use rustmd::menu::ToggleKeyMode;
use rustmd::window::{window_shadow, CloseWindow, MinimizeWindow, ZoomWindow};
use windows::Win32::UI::WindowsAndMessaging::{ShowWindowAsync, SW_RESTORE};

fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = Config::parse();

    Application::new().run(|cx: &mut App| {
        cx.activate(true);

        let theme = EditorTheme::dracula();
        cx.set_global(theme.clone());

        let initial_path = rustmd::file_ops::initial_file_path(&config);
        let content = rustmd::file_ops::initial_content(&config);

        cx.set_global(config);
        cx.set_global(CursorScreenPosition::default());
        cx.set_global(FileInfo {
            path: initial_path.clone(),
            dirty: false,
        });
        cx.set_global(StatusBarInfo::default());
        cx.set_global(KeyMode::default());

        cx.bind_keys([
            KeyBinding::new("ctrl-o", OpenFile, None),
            KeyBinding::new("ctrl-s", Save, None),
            KeyBinding::new("ctrl-shift-s", SaveAs, None),
            KeyBinding::new("ctrl-alt-n", NewFile, None),
        ]);

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let file_path_for_watcher = initial_path.clone();

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(900.0), px(700.0)),
                ))),
                window_decorations: Some(WindowDecorations::Client),
                titlebar: Some(TitlebarOptions {
                    title: None,
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                ..Default::default()
            },
            |window, cx| {
                let editor = cx.new(|cx| {
                    let mut editor = Editor::new(&content, cx);
                    if let Some(path) = file_path_for_watcher {
                        editor.watch_file(path, cx);
                    }
                    editor
                });
                window.focus(&editor.read(cx).focus_handle(cx));
                cx.new(|_cx| RootView { editor })
            },
        )
        .unwrap();
    });
}

struct RootView {
    editor: Entity<Editor>,
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = EditorTheme::global(cx).clone();

        window_shadow(theme)
            .child(
                div()
                    .size_full()
                    .on_action(|_: &MinimizeWindow, window, _cx| {
                        window.minimize_window();
                    })
                    .on_action(|_: &ZoomWindow, window, _cx| {
                        if window.is_maximized() {
                            if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) {
                                if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                                    unsafe {
                                        let hwnd = windows::Win32::Foundation::HWND(win32_handle.hwnd.get() as _);
                                        let _ = ShowWindowAsync(hwnd, SW_RESTORE);
                                    }
                                }
                            }
                        } else {
                            window.zoom_window();
                        }
                    })
                    .on_action(cx.listener(
                        |this: &mut RootView, _: &CloseWindow, window, cx| {
                            let editor = this.editor.clone();
                            if editor.read(cx).is_dirty() {
                                rustmd::file_ops::set_dialog_open(true);
                                window.defer(cx, move |window, cx| {
                                    let should_close = editor.update(cx, |editor, cx| {
                                        match rustmd::file_ops::confirm_discard() {
                                            rustmd::file_ops::DiscardChoice::Save => {
                                                editor.save(cx);
                                                !editor.is_dirty()
                                            }
                                            rustmd::file_ops::DiscardChoice::Cancel => false,
                                            rustmd::file_ops::DiscardChoice::DontSave => true,
                                        }
                                    });
                                    rustmd::file_ops::set_dialog_open(false);
                                    if should_close {
                                        window.remove_window();
                                    }
                                });
                            } else {
                                window.remove_window();
                            }
                        },
                    ))
                    .on_action(|_: &ToggleKeyMode, _window, cx| {
                        KeyMode::toggle(cx);
                        cx.refresh_windows();
                    })
                    .child(
                        div()
                            .size_full()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex_1()
                                    .child(EditorImeElement::new(self.editor.clone()))
                            )
                    )
            )
    }
}
