use clap::Parser;
use gpui::*;
use gpui::prelude::FluentBuilder;
use raw_window_handle::RawWindowHandle;
use rustmd::config::Config;
use rustmd::editor::ime::EditorImeElement;
use rustmd::editor::{CenterLine, Editor, EditorConfig, EditorTheme};
use rustmd::file_ops::{NewFile, OpenFile, Save, SaveAs};
use rustmd::key_mode::KeyMode;
use rustmd::status_bar::status_bar;
use rustmd::title_bar::{title_bar, FileInfo};
use rustmd::menu::{ToggleAbout, ToggleKeyMode};
use rustmd::user_config;
use rustmd::window::{window_shadow, CloseWindow, MinimizeWindow, NewWindow, ZoomWindow};
use windows::Win32::UI::WindowsAndMessaging::{ShowWindowAsync, SW_RESTORE};

fn main() {

    let config = Config::parse();

    Application::new().run(|cx: &mut App| {
        cx.activate(true);

        let user_cfg = user_config::load_config();
        let theme = user_cfg.theme.to_editor_theme();
        cx.set_global(theme.clone());

        let initial_path = rustmd::file_ops::initial_file_path(&config);
        let content = rustmd::file_ops::initial_content(&config);

        let editor_config = EditorConfig {
            text_font: user_cfg.text_font.clone(),
            code_font: user_cfg.code_font.clone(),
            theme: user_cfg.theme.to_editor_theme(),
            ..Default::default()
        };

        cx.set_global(config);
        cx.set_global(KeyMode::default());

        cx.bind_keys([
            KeyBinding::new("ctrl-o", OpenFile, None),
            KeyBinding::new("ctrl-s", Save, None),
            KeyBinding::new("ctrl-shift-s", SaveAs, None),
            KeyBinding::new("ctrl-alt-n", NewFile, None),
            KeyBinding::new("ctrl-shift-n", NewWindow, None),
            KeyBinding::new("ctrl-l", CenterLine, None),
        ]);

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let file_path_for_watcher = initial_path.clone();

        let win_size = size(px(900.0), px(700.0));
        let win_pos = cx.primary_display().map_or(point(px(0.), px(0.)), |d| {
            let b = d.bounds();
            point(
                b.origin.x + (b.size.width - win_size.width) / 2.0,
                b.origin.y + (b.size.height - win_size.height) / 2.0,
            )
        });

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    win_pos,
                    win_size,
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
                let handle = window.window_handle();
                let editor = cx.new(|cx| {
                    let mut editor = Editor::with_config(&content, editor_config, cx);
                    if let Some(path) = file_path_for_watcher {
                        editor.watch_file(path, cx);
                    }
                    editor
                });
                editor.update(cx, |editor, cx| {
                    editor.start_cursor_blink(handle, cx);
                });
                window.focus(&editor.read(cx).focus_handle(cx));
                cx.new(|_cx| RootView {
                    editor,
                    file_info: FileInfo {
                        path: initial_path.clone(),
                        dirty: false,
                    },
                    about_open: false,
                })
            },
        )
        .unwrap();
    });
}

fn open_new_window(cx: &mut App) {
    let user_cfg = user_config::load_config();
    let editor_config = EditorConfig {
        text_font: user_cfg.text_font.clone(),
        code_font: user_cfg.code_font.clone(),
        theme: user_cfg.theme.to_editor_theme(),
        ..Default::default()
    };

    let win_size = size(px(900.0), px(700.0));
    let win_pos = cx.primary_display().map_or(point(px(0.), px(0.)), |d| {
        let b = d.bounds();
        point(
            b.origin.x + (b.size.width - win_size.width) / 2.0,
            b.origin.y + (b.size.height - win_size.height) / 2.0,
        )
    });

    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(win_pos, win_size))),
            window_decorations: Some(WindowDecorations::Client),
            titlebar: Some(TitlebarOptions {
                title: None,
                appears_transparent: true,
                traffic_light_position: None,
            }),
            ..Default::default()
        },
        |window, cx| {
            let handle = window.window_handle();
            let editor = cx.new(|cx| Editor::with_config("", editor_config, cx));
            editor.update(cx, |editor, cx| {
                editor.start_cursor_blink(handle, cx);
            });
            window.focus(&editor.read(cx).focus_handle(cx));
            cx.new(|_cx| RootView {
                editor,
                file_info: FileInfo {
                    path: None,
                    dirty: false,
                },
                about_open: false,
            })
        },
    )
    .unwrap();
}

struct RootView {
    editor: Entity<Editor>,
    file_info: FileInfo,
    about_open: bool,
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = EditorTheme::global(cx).clone();
        let config = Config::global(cx).clone();

        let editor = self.editor.read(cx);
        self.file_info.path = editor.file_path().cloned();
        self.file_info.dirty = editor.is_dirty();
        let status_info = editor.status_info().clone();
        let _ = editor;

        window_shadow(theme.clone())
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
                    .on_action(cx.listener(|_: &mut RootView, _: &NewWindow, _window, cx| {
                        open_new_window(cx);
                    }))
                    .on_action(cx.listener(|this: &mut RootView, _: &ToggleAbout, _window, _cx| {
                        this.about_open = !this.about_open;
                    }))
                    .flex()
                    .flex_col()
                    .child(title_bar(&theme, &self.file_info, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .child(EditorImeElement::new(self.editor.clone())),
                    )
                    .child(status_bar(&status_info, &theme, &config))
                    .when(self.about_open, |parent| {
                        parent
                            .child(
                                // Overlay catches clicks outside popover to close it
                                div()
                                    .absolute()
                                    .size_full()
                                    .top_0()
                                    .left_0()
                                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                        window.dispatch_action(ToggleAbout.boxed_clone(), cx);
                                    })
                            )
                            .child(
                                // Popover — renders on top of overlay
                                div()
                                    .absolute()
                                    .top(rems(2.5))
                                    .left(rems(1.5))
                                    .bg(theme.background)
                                    .border_1()
                                    .border_color(theme.comment)
                                    .rounded(px(6.0))
                                    .py(rems(1.0))
                                    .px(rems(1.5))
                                    .child(format!("\u{1F980} rustmd v{}", env!("CARGO_PKG_VERSION")))
                                    .child(
                                        div()
                                            .text_color(theme.comment)
                                            .text_xs()
                                            .child("based on writ editor")
                                    )
                                    .child(
                                        div()
                                            .mt(rems(0.5))
                                            .pt(rems(0.5))
                                            .border_t_1()
                                            .border_color(theme.selection)
                                            .text_color(theme.cyan)
                                            .cursor_pointer()
                                            .hover(|s| s.opacity(0.7))
                                            .on_mouse_down(MouseButton::Left, |_, _, _cx| {
                                                let path = rustmd::user_config::config_path();
                                                if let Some(parent) = path.parent() {
                                                    let _ = open::that(parent);
                                                }
                                            })
                                            .child("Open Config Directory →")
                                    )
                            )
                    }),
            )
    }
}
