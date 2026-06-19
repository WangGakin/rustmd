#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use clap::Parser;
use gpui::*;
use gpui::prelude::FluentBuilder;
use raw_window_handle::RawWindowHandle;
use rustmd::config::Config;
use rustmd::editor::ime::EditorImeElement;
use rustmd::editor::{CenterLine, Editor, EditorConfig, EditorTheme};
use rustmd::file_ops::{ClearRecentFiles, NewFile, OpenFile, OpenRecentFile, Save, SaveAs};
use rustmd::file_explorer::{self, ExplorerNextPage, ExplorerPrevPage, OpenExplorerFile, ToggleFileExplorer};
use rustmd::key_mode::KeyMode;
use rustmd::status_bar::status_bar;
use rustmd::title_bar::{title_bar, FileInfo, ToggleRecentFiles};
use rustmd::menu::{ToggleAbout, ToggleKeyMode};
use rustmd::tooltip::Tooltip;
use rustmd::user_config;
use rustmd::window::{window_shadow, CloseWindow, MinimizeWindow, NewWindow, ZoomWindow};
use windows::Win32::UI::WindowsAndMessaging::{ShowWindowAsync, SW_RESTORE};

fn main() {
    env_logger::init();

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
        cx.set_global(Tooltip { text: None });

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

        let win_size = size(px(rustmd::config::DEFAULT_WIN_WIDTH), px(rustmd::config::DEFAULT_WIN_HEIGHT));
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
                if let Some(ref path) = initial_path {
                    rustmd::user_config::add_recent_file(path);
                }
                editor.update(cx, |editor, cx| {
                    editor.start_cursor_blink(handle, cx);
                });
                window.focus(&editor.read(cx).focus_handle(cx));
                cx.new(|_cx| {
                    let files = rustmd::user_config::recent_files();
                    RootView {
                        editor,
                        file_info: FileInfo {
                            path: initial_path.clone(),
                            dirty: false,
                            recent_files: files.clone(),
                        },
                        about_open: false,
                        recent_files_open: false,
                        recent_files: files,
                        file_explorer_open: false,
                        explorer_files: Vec::new(),
                        explorer_page: 0,
                    }
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

    let win_size = size(px(rustmd::config::DEFAULT_WIN_WIDTH), px(rustmd::config::DEFAULT_WIN_HEIGHT));
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
            cx.new(|_cx| {
                let files = rustmd::user_config::recent_files();
                RootView {
                    editor,
                    file_info: FileInfo {
                        path: None,
                        dirty: false,
                        recent_files: files.clone(),
                    },
                    about_open: false,
                    recent_files_open: false,
                    recent_files: files,
                    file_explorer_open: false,
                    explorer_page: 0,
                    explorer_files: Vec::new(),
                }
            })
        },
    )
    .unwrap();
}

fn open_new_window_with_file(path: PathBuf, cx: &mut App) {
    let user_cfg = user_config::load_config();
    let editor_config = EditorConfig {
        text_font: user_cfg.text_font.clone(),
        code_font: user_cfg.code_font.clone(),
        theme: user_cfg.theme.to_editor_theme(),
        ..Default::default()
    };

    let content = std::fs::read_to_string(&path).unwrap_or_default();
    user_config::add_recent_file(&path);

    let win_size = size(px(rustmd::config::DEFAULT_WIN_WIDTH), px(rustmd::config::DEFAULT_WIN_HEIGHT));
    let win_pos = cx.primary_display().map_or(point(px(0.), px(0.)), |d| {
        let b = d.bounds();
        point(
            b.origin.x + (b.size.width - win_size.width) / 2.0,
            b.origin.y + (b.size.height - win_size.height) / 2.0,
        )
    });

    let path_for_watcher = path.clone();

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
            let editor = cx.new(|cx| {
                let mut editor = Editor::with_config(&content, editor_config, cx);
                editor.watch_file(path_for_watcher, cx);
                editor
            });
            editor.update(cx, |editor, cx| {
                editor.start_cursor_blink(handle, cx);
            });
            window.focus(&editor.read(cx).focus_handle(cx));
            cx.new(|_cx| {
                let files = rustmd::user_config::recent_files();
                RootView {
                    editor,
                    file_info: FileInfo {
                        path: Some(path),
                        dirty: false,
                        recent_files: files.clone(),
                    },
                    about_open: false,
                    recent_files_open: false,
                    recent_files: files,
                    explorer_page: 0,
                    file_explorer_open: false,
                    explorer_files: Vec::new(),
                }
            })
        },
    )
    .unwrap();
}
struct RootView {
    editor: Entity<Editor>,
    file_info: FileInfo,
    about_open: bool,
    recent_files_open: bool,
    recent_files: Vec<String>,
    file_explorer_open: bool,
    explorer_files: Vec<PathBuf>,
    explorer_page: usize,
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = EditorTheme::global(cx).clone();
        let config = Config::global(cx).clone();
        let tooltip = Tooltip::global(cx).clone();

        let editor = self.editor.read(cx);
        self.file_info.path = editor.file_path().cloned();
        self.file_info.dirty = editor.is_dirty();
        let status_info = editor.status_info().clone();

        // Only refresh from global state when the popup is open (user-initiated action).
        // On idle frames we use the cached copy to avoid Mutex lock + Vec clone.
        if self.recent_files_open {
            self.recent_files = rustmd::user_config::recent_files();
            self.file_info.recent_files.clone_from(&self.recent_files);
        }
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
                            if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window)
                                && let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                                    unsafe {
                                        let hwnd = windows::Win32::Foundation::HWND(win32_handle.hwnd.get() as _);
                                        let _ = ShowWindowAsync(hwnd, SW_RESTORE);
                                    }
                                }
                        } else {
                            window.zoom_window();
                        }
                    })
                    .on_action(cx.listener(
                        |this: &mut RootView, _: &CloseWindow, window, cx| {
                             if this.editor.read(cx).is_dirty() {
                                 let weak = this.editor.downgrade();
                                 let win_handle = window.window_handle();
                                 cx.spawn(async move |_tx, cx| {
                                     let choice = rustmd::file_ops::confirm_discard();
                                     rustmd::file_ops::set_dialog_open(false);

                                     let should_close = match choice {
                                         rustmd::file_ops::DiscardChoice::Save => {
                                            cx.update(|cx| {
                                                if let Some(editor) = weak.upgrade() {
                                                    editor.update(cx, |editor, cx| {
                                                        editor.save(cx);
                                                        !editor.is_dirty()
                                                    })
                                                } else {
                                                    false
                                                }
                                            }).ok().unwrap_or(false)
                                         }
                                         rustmd::file_ops::DiscardChoice::Cancel => false,
                                         rustmd::file_ops::DiscardChoice::DontSave => true,
                                     };

                                    if should_close
                                        && cx.update_window(win_handle, |_, window, _cx| {
                                            window.remove_window();
                                        }).is_err()
                                    {
                                        log::error!("CloseWindow: window not found during async removal");
                                    }
                                 })
                                 .detach();
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
                    .on_action(cx.listener(
                        |this: &mut RootView, _: &OpenFile, window, cx| {
                            let is_pristine = this.editor.read(cx).file_path().is_none()
                                && !this.editor.read(cx).is_dirty();
                            if is_pristine {
                                let weak = this.editor.downgrade();
                                let win_handle = window.window_handle();
                                cx.spawn(async move |_tx, cx| {
                                    let path = rustmd::file_ops::pick_open_file();
                                    if let Some(path) = path {
                                        let _ = cx.update(|cx| {
                                            if let Some(editor) = weak.upgrade() {
                                                editor.update(cx, |editor, cx| {
                                                    editor.open_file_at(path, cx);
                                                })
                                            }
                                        });
                                        let _ = cx.update_window(win_handle, |_, window, _cx| {
                                            window.refresh();
                                        });
                                    }
                                })
                                .detach();
                            } else {
                                let _ = window.window_handle();
                                cx.spawn(async move |_tx, cx| {
                                    let path = rustmd::file_ops::pick_open_file();
                                    if let Some(path) = path {
                                        let _ = cx.update(|cx| {
                                            open_new_window_with_file(path, cx);
                                        });
                                    }
                                })
                                .detach();
                            }
                        },
                    ))
                    .on_action(cx.listener(|this: &mut RootView, _: &ToggleAbout, _window, _cx| {
                        this.about_open = !this.about_open;
                    }))
                    .on_action(cx.listener(
                        |this: &mut RootView, _: &ToggleRecentFiles, _window, _cx| {
                            this.recent_files_open = !this.recent_files_open;
                        },
                    ))
                    .on_action(cx.listener(
                        |this: &mut RootView, _: &ToggleFileExplorer, _window, cx| {
                            this.file_explorer_open = !this.file_explorer_open;
                            if this.file_explorer_open {
                                let folder = this
                                    .editor
                                    .read(cx)
                                    .file_path()
                                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                                this.explorer_files = file_explorer::scan_folder(&folder);
                                this.explorer_page = 0;
                            }
                        },
                    ))
                    .on_action(cx.listener(
                        |this: &mut RootView, _: &ExplorerPrevPage, _window, _cx| {
                            this.explorer_page = this.explorer_page.saturating_sub(1);
                        },
                    ))
                    .on_action(cx.listener(
                        |this: &mut RootView, _: &ExplorerNextPage, _window, _cx| {
                            let total = this.explorer_files.len().div_ceil(file_explorer::PAGE_SIZE);
                            if this.explorer_page + 1 < total {
                                this.explorer_page += 1;
                            }
                        },
                    ))
                    .on_action(cx.listener(
                        |this: &mut RootView, action: &OpenRecentFile, window, cx| {
                            let index = action.0;
                            let Some(path_str) = this.recent_files.get(index) else {
                                return;
                            };
                            let path = std::path::PathBuf::from(path_str);
                            let weak = this.editor.downgrade();
                            let is_dirty = this.editor.read(cx).is_dirty();
                            let win_handle = window.window_handle();
                            cx.spawn(async move |_tx, cx| {
                                let (should_open, needs_save) = if is_dirty {
                                    match rustmd::file_ops::confirm_discard() {
                                        rustmd::file_ops::DiscardChoice::Save => (true, true),
                                        rustmd::file_ops::DiscardChoice::Cancel => (false, false),
                                        rustmd::file_ops::DiscardChoice::DontSave => (true, false),
                                    }
                                } else {
                                    (true, false)
                                };

                                if !should_open {
                                    return;
                                }

                                if needs_save {
                                    let _ = cx.update(|cx| {
                                        if let Some(editor) = weak.upgrade() {
                                            editor.update(cx, |editor, cx| {
                                                editor.save(cx);
                                                editor.is_dirty()
                                            })
                                        } else {
                                            false
                                        }
                                    });
                                    let still_dirty: bool = cx.update(|cx| {
                                        if let Some(e) = weak.upgrade() {
                                            e.read(cx).is_dirty()
                                        } else {
                                            true
                                        }
                                    }).ok().unwrap_or(true);
                                    if still_dirty {
                                        return;
                                    }
                                }

                                let _ = cx.update(|cx| {
                                    if let Some(editor) = weak.upgrade() {
                                        editor.update(cx, |editor, cx| {
                                            editor.open_file_at(path, cx);
                                        })
                                    }
                                });
                                let _ = cx.update_window(win_handle, |_, window, cx| {
                                    window.refresh();
                                    window.dispatch_action(ToggleRecentFiles.boxed_clone(), cx);
                                });
                            })
                            .detach();
                        },
                    ))
                    .on_action(cx.listener(
                        |this: &mut RootView, action: &OpenExplorerFile, window, cx| {
                            let path = action.0.clone();
                            let weak = this.editor.downgrade();
                            let is_dirty = this.editor.read(cx).is_dirty();
                            let win_handle = window.window_handle();
                            cx.spawn(async move |_tx, cx| {
                                let (should_open, needs_save) = if is_dirty {
                                    match rustmd::file_ops::confirm_discard() {
                                        rustmd::file_ops::DiscardChoice::Save => (true, true),
                                        rustmd::file_ops::DiscardChoice::Cancel => (false, false),
                                        rustmd::file_ops::DiscardChoice::DontSave => (true, false),
                                    }
                                } else {
                                    (true, false)
                                };

                                if !should_open {
                                    return;
                                }

                                if needs_save {
                                    let _ = cx.update(|cx| {
                                        if let Some(editor) = weak.upgrade() {
                                            editor.update(cx, |editor, cx| {
                                                editor.save(cx);
                                                editor.is_dirty()
                                            })
                                        } else {
                                            false
                                        }
                                    });
                                    let still_dirty: bool = cx.update(|cx| {
                                        if let Some(e) = weak.upgrade() {
                                            e.read(cx).is_dirty()
                                        } else {
                                            true
                                        }
                                    }).ok().unwrap_or(true);
                                    if still_dirty {
                                        return;
                                    }
                                }

                                let _ = cx.update(|cx| {
                                    if let Some(editor) = weak.upgrade() {
                                        editor.update(cx, |editor, cx| {
                                            editor.open_file_at(path, cx);
                                        })
                                    }
                                });
                                let _ = cx.update_window(win_handle, |_, window, cx| {
                                    window.refresh();
                                    window.dispatch_action(ToggleFileExplorer.boxed_clone(), cx);
                                });
                            })
                            .detach();
                        },
                    ))
                    .on_action(cx.listener(
                        |this: &mut RootView, _: &ClearRecentFiles, _window, _cx| {
                            rustmd::user_config::clear_recent_files();
                            this.recent_files_open = false;
                        },
                    ))
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
                    .when_some(tooltip.text.clone(), |parent, text| {
                        parent.child(
                            div()
                                .absolute()
                                .top(rems(0.25))
                                .left(px(0.0))
                                .px(rems(0.5))
                                .py(rems(0.15))
                                .text_xs()
                                .text_color(theme.background)
                                .bg(theme.foreground)
                                .rounded(px(4.0))
                                .child(text),
                        )
                    })
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
                                // Popover renders on top of overlay
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
                                            .child("Open Config Directory \u{2192}")
                                    )
                            )
                    })
                    .when(self.recent_files_open, |parent| {
                        let theme_clone = theme.clone();
                        let files = self.recent_files.clone();
                        parent
                            .child(
                                div()
                                    .absolute()
                                    .size_full()
                                    .top_0()
                                    .left_0()
                                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                        window.dispatch_action(ToggleRecentFiles.boxed_clone(), cx);
                                    })
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(rems(2.5))
                                    .right(rems(0.5))
                                    .w(rems(18.0))
                                    .bg(theme_clone.background)
                                    .border_1()
                                    .border_color(theme_clone.comment)
                                    .rounded(px(4.0))
                                    .py(rems(0.25))
                                    .flex()
                                    .flex_col()
                                    .children(
                                        files.iter().enumerate().map(|(i, path_str)| {
                                            let p = std::path::Path::new(path_str);
                                            let name = p
                                                .file_name()
                                                .map(|n| n.to_string_lossy().to_string())
                                                .unwrap_or_else(|| path_str.clone());
                                            let parent_name = p
                                                .parent()
                                                .and_then(|p| p.file_name())
                                                .map(|n| n.to_string_lossy().to_string())
                                                .unwrap_or_default();
                                            let label = if parent_name.is_empty() {
                                                name.clone()
                                            } else {
                                                format!("{} \u{2014} {}", name, parent_name)
                                            };
                                            let exists = p.exists();
                                            let action = OpenRecentFile(i).boxed_clone();
                                            div()
                                                .px(rems(1.0))
                                                .py(rems(0.3))
                                                .text_color(theme_clone.foreground)
                                                .when(!exists, |this| {
                                                    this.text_color(theme_clone.comment)
                                                })
                                                .cursor_pointer()
                                                .hover(|s| s.bg(theme_clone.selection))
                                                .child(label)
                                                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                                    window.dispatch_action(action.boxed_clone(), cx);
                                                })
                                                .into_any_element()
                                        }).collect::<Vec<_>>()
                                    )
                                    .when(!files.is_empty(), |this| {
                                        this.child(
                                            div()
                                                .mx(rems(1.0))
                                                .border_t_1()
                                                .border_color(theme_clone.selection)
                                        )
                                    })
                                    .child(
                                        div()
                                            .px(rems(1.0))
                                            .py(rems(0.3))
                                            .text_color(theme_clone.comment)
                                            .cursor_pointer()
                                            .hover(|s| s.opacity(0.7))
                                            .child("Clear Recent Files")
                                            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                                window.dispatch_action(ClearRecentFiles.boxed_clone(), cx);
                                            })
                                    ),
                            )
                    })
                    .when(self.file_explorer_open, |parent| {
                        let theme_clone = theme.clone();
                        let folder = self.file_info.path.as_ref()
                            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                        let files = self.explorer_files.clone();
                        let current = self.file_info.path.clone();
                        parent
                            .child(
                                div()
                                    .absolute()
                                    .size_full()
                                    .top_0()
                                    .left_0()
                                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                        window.dispatch_action(ToggleFileExplorer.boxed_clone(), cx);
                                    })
                            )
                            .child(
                                file_explorer::file_explorer_panel(
                                    &folder,
                                    &files,
                                    current.as_ref(),
                                    self.explorer_page,
                                    &theme_clone,
                                )
                            )
                    }),
            )
    }
}
