# Recent Files Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "recent files" dropdown in the title bar that tracks the last 5 opened files with persistence in `config.json`.

**Architecture:** Extend `UserConfig` with `recent_files: Vec<String>`, use a `static LazyLock<Mutex<Vec<String>>>` for in-memory caching, hook into existing `open_file`/`save_as`/`--file` flows to record paths, and add a popover-styled dropdown on the right side of the title bar following the existing `ToggleAbout` overlay pattern.

**Tech Stack:** Rust, GPUI 0.2, serde_json, rfd

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/user_config.rs` | Modify | Add `recent_files` field, static cache, `add_recent_file()`, `clear_recent_files()`, `recent_files()` |
| `src/file_ops.rs` | Modify | Add `OpenRecentFile(usize)` and `ClearRecentFiles` action structs |
| `src/editor/mod.rs` | Modify | Extract `open_file_at(path)`, call `add_recent_file` in `open_file`, `save_as`, `open_file_at` |
| `src/title_bar.rs` | Modify | Add `recent_files` to `FileInfo`, render recent files button in title bar |
| `src/main.rs` | Modify | Import new actions, bind handlers, add `recent_files_open` state, render dropdown popover, record `--file` path |

---

### Task 1: Data model and persistence helpers

**Files:**
- Modify: `src/user_config.rs`

- [ ] **Step 1: Add `recent_files` field to `UserConfig`**

Add the field after `font_size_rem`:

```rust
/// GUI user preferences persisted as JSON.
#[derive(Serialize, Deserialize)]
pub struct UserConfig {
    #[serde(default)]
    pub theme: SerializedTheme,
    #[serde(default = "default_text_font")]
    pub text_font: String,
    #[serde(default = "default_code_font")]
    pub code_font: String,
    #[serde(default = "default_font_size")]
    pub font_size_rem: f32,
    #[serde(default)]
    pub recent_files: Vec<String>,
}
```

- [ ] **Step 2: Add static cache and helper functions**

Add after the `use` statements at the top of `user_config.rs`:

```rust
use std::path::Path;
use std::sync::{LazyLock, Mutex};
```

Add at the bottom of the file, before `fn default_font_size()`:

```rust
static RECENT_FILES: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| {
    Mutex::new(load_config().recent_files)
});

pub fn add_recent_file(path: &Path) {
    let path_str = path.to_string_lossy().to_string();
    if path_str.is_empty() {
        return;
    }
    let mut files = RECENT_FILES.lock().unwrap();
    files.retain(|f| f != &path_str);
    files.insert(0, path_str);
    files.truncate(5);
    // persist to config.json
    let mut cfg = load_config();
    cfg.recent_files = files.clone();
    save_config(&cfg);
}

pub fn clear_recent_files() {
    let mut files = RECENT_FILES.lock().unwrap();
    files.clear();
    let mut cfg = load_config();
    cfg.recent_files.clear();
    save_config(&cfg);
}

pub fn recent_files() -> Vec<String> {
    RECENT_FILES.lock().unwrap().clone()
}
```

- [ ] **Step 3: Build to verify compilation**

```bash
cargo check 2>&1
```

Expected: compiles cleanly.

---

### Task 2: New action structs

**Files:**
- Modify: `src/file_ops.rs`

- [ ] **Step 1: Add `OpenRecentFile` and `ClearRecentFiles` action structs**

After the existing `actions!` macro call (line 9), add:

```rust
use gpui::Action;

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct OpenRecentFile(pub usize);

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct ClearRecentFiles;
```

- [ ] **Step 2: Build to verify**

```bash
cargo check 2>&1
```

Expected: compiles cleanly.

---

### Task 3: Refactor `open_file` + record recent files

**Files:**
- Modify: `src/editor/mod.rs`

- [ ] **Step 1: Extract `open_file_at()` method**

Insert this new method right before `open_file()` (around line 3149):

```rust
    /// Open a file at the given path, replacing current content.
    pub fn open_file_at(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to open file: {}", e);
                return;
            }
        };

        self.file_path = None;
        self.file_watcher = None;
        self.file_watcher_rx = None;

        self.set_text(&content, cx);
        self.state.buffer.mark_clean();
        self.file_path = Some(path.clone());
        self.watch_file(path.clone(), cx);

        crate::user_config::add_recent_file(&path);

        cx.notify();
    }
```

- [ ] **Step 2: Refactor `open_file()` to delegate to `open_file_at()`**

Replace the body of `open_file()` (lines 3150-3181) with:

```rust
    /// Open a file chosen via file dialog, replacing current content.
    pub fn open_file(&mut self, cx: &mut Context<Self>) {
        if self.state.buffer.is_dirty() {
            match crate::file_ops::confirm_discard() {
                crate::file_ops::DiscardChoice::Save => self.save(cx),
                crate::file_ops::DiscardChoice::Cancel => return,
                crate::file_ops::DiscardChoice::DontSave => {}
            }
        }

        let Some(path) = crate::file_ops::pick_open_file() else {
            return;
        };
        self.open_file_at(path, cx);
    }
```

- [ ] **Step 3: Record in `save_as()`**

After the line `self.file_path = Some(path.clone());` in `save_as()` (currently line 3138), add:

```rust
        crate::user_config::add_recent_file(&path);
```

- [ ] **Step 4: Build to verify**

```bash
cargo check 2>&1
```

Expected: compiles cleanly.

---

### Task 4: Title bar button and dropdown

**Files:**
- Modify: `src/title_bar.rs`

- [ ] **Step 1: Add `recent_files` and `recent_files_open` to `FileInfo`**

Replace the `FileInfo` struct:

```rust
pub struct FileInfo {
    pub path: Option<std::path::PathBuf>,
    pub dirty: bool,
    pub recent_files: Vec<String>,
    pub recent_files_open: bool,
}
```

- [ ] **Step 2: Add `ToggleRecentFiles` action**

Add the action after the existing imports, before `FileInfo`:

```rust
use gpui::{Action, App, ElementId, Fill, MouseButton, div, prelude::*, rems};
```

And add:

```rust
#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct ToggleRecentFiles;
```

- [ ] **Step 3: Modify `title_bar()` to render recent files button**

Replace the entire `title_bar` function. The key changes are:
- Accept `file_info: &FileInfo` (already does)
- Add a recent files button between the filename area and traffic lights
- Empty list → greyed out, non-empty → clickable

```rust
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
```

- [ ] **Step 4: Build to verify**

```bash
cargo check 2>&1
```

Expected: compiles cleanly (may have unused import warnings for `ToggleRecentFiles` until Task 5).

---

### Task 5: Wire up RootView in main.rs

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Update imports**

Replace the existing import block from `rustmd::file_ops` line:

```rust
use rustmd::file_ops::{NewFile, OpenFile, Save, SaveAs};
```

With:

```rust
use rustmd::file_ops::{ClearRecentFiles, NewFile, OpenFile, OpenRecentFile, Save, SaveAs};
```

Add title_bar import for `ToggleRecentFiles`:

```rust
use rustmd::title_bar::{title_bar, FileInfo, ToggleRecentFiles};
```

- [ ] **Step 2: Add `recent_files_open` to `RootView`**

Replace `RootView` struct:

```rust
struct RootView {
    editor: Entity<Editor>,
    file_info: FileInfo,
    about_open: bool,
    recent_files_open: bool,
}
```

- [ ] **Step 3: Populate `file_info.recent_files` in render**

In `RootView::render()`, after `self.file_info.dirty = editor.is_dirty();` (line 176), add:

```rust
        self.file_info.recent_files = rustmd::user_config::recent_files();
        self.file_info.recent_files_open = self.recent_files_open;
```

- [ ] **Step 4: Add action handlers to the outer div in `RootView::render()`**

Add the following `.on_action` handlers inside the outer `div()` chain in `render()`, alongside the existing ones (e.g., after the `ToggleKeyMode` handler at line 227):

```rust
                    .on_action(cx.listener(
                        |this: &mut RootView, _: &ToggleRecentFiles, _window, _cx| {
                            this.recent_files_open = !this.recent_files_open;
                        },
                    ))
                    .on_action(cx.listener(
                        |this: &mut RootView, action: &OpenRecentFile, _window, cx| {
                            let index = action.0;
                            let files = rustmd::user_config::recent_files();
                            if let Some(path_str) = files.get(index) {
                                let path = std::path::PathBuf::from(path_str);
                                let editor = this.editor.clone();
                                let should_open = editor.update(cx, |editor, cx| {
                                    if editor.is_dirty() {
                                        match rustmd::file_ops::confirm_discard() {
                                            rustmd::file_ops::DiscardChoice::Save => {
                                                editor.save(cx);
                                                true
                                            }
                                            rustmd::file_ops::DiscardChoice::Cancel => false,
                                            rustmd::file_ops::DiscardChoice::DontSave => true,
                                        }
                                    } else {
                                        true
                                    }
                                });
                                if should_open {
                                    editor.update(cx, |editor, cx| {
                                        editor.open_file_at(path, cx);
                                    });
                                }
                            }
                            this.recent_files_open = false;
                            cx.notify();
                        },
                    ))
                    .on_action(cx.listener(
                        |this: &mut RootView, _: &ClearRecentFiles, _window, _cx| {
                            rustmd::user_config::clear_recent_files();
                            this.recent_files_open = false;
                        },
                    ))
```

- [ ] **Step 5: Add recent files dropdown popover in `RootView::render()`**

After the About popover `.when(self.about_open, ...)` block (ending around line 312), add a second `.when()`:

```rust
                    .when(self.recent_files_open, |parent| {
                        let theme_clone = theme.clone();
                        let files = rustmd::user_config::recent_files();
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
                    }),
```

- [ ] **Step 6: Record recent file on `--file` startup**

After the line `editor.watch_file(path, cx);` (line 91 in `main.rs`), add:

```rust
                        rustmd::user_config::add_recent_file(&path);
```

The block should look like:

```rust
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
```

- [ ] **Step 7: Update initial `RootView` creation to include `recent_files_open`**

In the `RootView` creation in `main` (line 99):

```rust
                cx.new(|_cx| RootView {
                    editor,
                    file_info: FileInfo {
                        path: initial_path.clone(),
                        dirty: false,
                        recent_files: rustmd::user_config::recent_files(),
                        recent_files_open: false,
                    },
                    about_open: false,
                    recent_files_open: false,
                })
```

Also update `open_new_window` (line 149):

```rust
            cx.new(|_cx| RootView {
                editor,
                file_info: FileInfo {
                    path: None,
                    dirty: false,
                    recent_files: rustmd::user_config::recent_files(),
                    recent_files_open: false,
                },
                about_open: false,
                recent_files_open: false,
            })
```

- [ ] **Step 8: Build and verify**

```bash
cargo check 2>&1
```

Expected: compiles cleanly.

- [ ] **Step 9: Commit all changes**

```bash
git add src/user_config.rs src/file_ops.rs src/editor/mod.rs src/title_bar.rs src/main.rs
git commit -m "feat: add recent files dropdown in title bar"
```

---

### Verification

- [ ] **Build:** `cargo build --release` succeeds
- [ ] **Functional test:**
  1. Launch rustmd
  2. Open a file via Ctrl+O → file should appear in dropdown
  3. Open another file → both files in dropdown, most recent first
  4. Click a recent file in dropdown → opens with unsaved-check dialog if dirty
  5. Click "Clear Recent Files" → list clears, button greys out
  6. Restart app → recent files persist
  7. Launch with `rustmd --file some.md` → some.md recorded
- [ ] **Edge:**
  1. Non-existent file in list → greyed out but still clickable (shows error in console)
  2. Open same file twice → only one entry, moved to top
  3. Open 6 files → oldest removed, max 5 kept
