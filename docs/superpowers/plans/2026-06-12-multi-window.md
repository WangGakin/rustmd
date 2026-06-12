# Multi-Window Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable multiple independent OS windows, each with its own document, toolbar icon, and keyboard shortcut.

**Architecture:** Eliminate per-window globals (`FileInfo`, `StatusBarInfo`, `CursorScreenPosition`) and pass data directly from `RootView` (per-window) to `title_bar` and `status_bar`. Extract window creation into a reusable factory function.

**Tech Stack:** Rust, GPUI 0.2

---

## File Structure Overview

| File | Responsibility |
|------|---------------|
| `src/window.rs` | `WindowShadow` (decorations only), `NewWindow` action |
| `src/title_bar.rs` | Title bar widget, FileInfo struct (no longer Global) |
| `src/status_bar.rs` | Status bar widget, StatusBarInfo struct (no longer Global) |
| `src/editor/mod.rs` | Editor widget, stores its own status_info + cursor_screen_pos, stores window_handle |
| `src/line.rs` | Line rendering, writes to Editor's CursorScreenPosition via shared Rc |
| `src/main.rs` | RootView controls full layout, window factory function |
| `src/menu.rs` | Icon-based toolbar with NewWindow button |
| `src/file_ops.rs` | Remove `update_file_info_global` / `update_file_info_from_editor` |

---

### Task 1: Add `status_info` and `window_handle` fields to Editor

**Files:**
- Modify: `src/editor/mod.rs`

- [ ] **Add fields to Editor struct**

Around line 1696, after the `is_primary` field, add:

```rust
    /// Whether this is the primary editor that updates global state (status bar, title bar).
    is_primary: bool,
    /// Per-editor status bar info (replaces global StatusBarInfo).
    status_info: StatusBarInfo,
    /// Window handle for async operations (replaces cx.windows().first()).
    window_handle: Option<AnyWindowHandle>,
```

Add the required import at the top of the file (search for existing gpui imports and add):

```rust
use gpui::AnyWindowHandle;
```

- [ ] **Initialize fields in `with_config`**

In the constructor at line 1757 area, add after `is_primary: true,`:

```rust
            is_primary: true,
            status_info: StatusBarInfo::default(),
            window_handle: None,
```

- [ ] **Add getter method**

After the `file_path()` method (around line 1810), add:

```rust
    pub fn status_info(&self) -> &StatusBarInfo {
        &self.status_info
    }
```

- [ ] **Change `start_cursor_blink` to `&mut self`**

Change line 1769 signature from:
```rust
    pub fn start_cursor_blink(&self, handle: AnyWindowHandle, cx: &mut Context<Self>) {
```
To:
```rust
    pub fn start_cursor_blink(&mut self, handle: AnyWindowHandle, cx: &mut Context<Self>) {
```

And add at the beginning of the function:
```rust
        self.window_handle = Some(handle);
```

- [ ] **Store status_info during render**

In the Editor's `Render` impl, at line 3933-3946, change from writing to global to storing on self:

Replace:
```rust
        if self.is_primary {
            let new_status_bar_info = StatusBarInfo {
                context_markers,
                heading_level,
                cursor_line: cursor_line + 1,
                cursor_col: cursor_col + 1,
                total_lines,
                first_visible_line,
                last_visible_line,
            };
            if new_status_bar_info != *StatusBarInfo::global(cx) {
                cx.set_global(new_status_bar_info);
            }
        }
```

With:
```rust
        if self.is_primary {
            self.status_info = StatusBarInfo {
                context_markers,
                heading_level,
                cursor_line: cursor_line + 1,
                cursor_col: cursor_col + 1,
                total_lines,
                first_visible_line,
                last_visible_line,
            };
        }
```

- [ ] **Build to verify**

```bash
cargo build 2>&1 | head -30
```

Expected: Build fails due to missing imports (`AnyWindowHandle`). Fix by ensuring `use gpui::AnyWindowHandle;` is present. Also `StatusBarInfo` needs to be imported (check if it's already imported).

Note: `StatusBarInfo` is from `crate::status_bar::StatusBarInfo`. Check current imports at the top of editor/mod.rs.

---

### Task 2: Store CursorScreenPosition on Editor via Rc<RefCell>

**Files:**
- Modify: `src/editor/mod.rs`
- Modify: `src/line.rs`

- [ ] **Add Rc<RefCell<CursorScreenPosition>> field to Editor**

Add after the `status_info` field:

```rust
    /// Shared cursor screen position (written by Line paint, read by autocomplete popup).
    cursor_screen_pos: Rc<RefCell<CursorScreenPosition>>,
```

Add import:
```rust
use std::cell::RefCell;
use std::rc::Rc;
```

Initialize in constructor:
```rust
            window_handle: None,
            cursor_screen_pos: Rc::new(RefCell::new(CursorScreenPosition::default())),
```

- [ ] **Pass cursor_screen_pos to Line in the list callback**

In Editor's `render` method, before the `line_list` div at line 4098, clone the Rc:

```rust
        let cursor_screen_pos = self.cursor_screen_pos.clone();
```

Then in the `build_line` closure (around line 4112-4168), add `csp` parameter and pass to `Line::new`:

Change the `build_line` closure signature from:
```rust
                let build_line = |snap: &RenderSnapshot,
                                  line_idx: usize,
                                  extra_styles: Vec<StyledRegion>,
                                  ...
                                  block_input: bool|
                 -> Line {
```

To (add parameter):
```rust
                let build_line = |snap: &RenderSnapshot,
                                  line_idx: usize,
                                  extra_styles: Vec<StyledRegion>,
                                  ...
                                  block_input: bool,
                                  csp: Option<Rc<RefCell<CursorScreenPosition>>>|
                 -> Line {
```

And in the `Line::new(...)` call, pass `csp` as the last argument.

Also in the actual call to `build_line` (later in the list callback), pass `Some(cursor_screen_pos.clone())`.

- [ ] **Add cursor_screen_pos field to Line struct**

In `src/line.rs`, add to Line struct (after `show_cursor: bool,`):

```rust
    /// Shared storage for cursor screen position (set during paint).
    cursor_screen_pos: Option<Rc<RefCell<CursorScreenPosition>>>,
```

Add imports at top of `line.rs`:
```rust
use std::cell::RefCell;
use std::rc::Rc;
```

- [ ] **Add parameter to Line::new**

In `Line::new`, add parameter `cursor_screen_pos: Option<Rc<RefCell<CursorScreenPosition>>>` and store it:

```rust
        Self {
            ...
            show_cursor,
            cursor_screen_pos,
        }
```

Update the `#[allow(clippy::too_many_arguments)]` if needed.

- [ ] **Write to cursor_screen_pos instead of global during paint**

In `line.rs`, line 1090, replace:
```rust
                cx.set_global(CursorScreenPosition {
                    position: Some(pos),
                    content_right_edge: Some(bounds.origin.x + bounds.size.width),
                });
```

With:
```rust
                if let Some(csp) = &self.cursor_screen_pos {
                    *csp.borrow_mut() = CursorScreenPosition {
                        position: Some(pos),
                        content_right_edge: Some(bounds.origin.x + bounds.size.width),
                    };
                }
```

- [ ] **Update render_autocomplete to read from self**

In `editor/mod.rs`, `render_autocomplete` function, line 2771, replace:
```rust
        let cursor_screen_pos = CursorScreenPosition::global(cx);
```

With:
```rust
        let cursor_screen_pos = self.cursor_screen_pos.borrow();
```

And adjust the subsequent code. Note that `cursor_screen_pos` is now a `Ref` guard, which implements `Deref` so `.position?` and `.content_right_edge` should work unchanged. BUT there's a lifetime issue — the `cursor_pos` borrows from the `Ref` guard but needs to outlive it. Change to:

```rust
        let cursor_screen_pos = self.cursor_screen_pos.borrow();
        let cursor_pos = cursor_screen_pos.position?;
        let content_right_edge = cursor_screen_pos.content_right_edge;
        drop(cursor_screen_pos);  // Release the borrow
```

Then change the usage of `cursor_screen_pos.content_right_edge` to `content_right_edge`.

- [ ] **Build**

```bash
cargo build 2>&1 | head -30
```

Fix any compilation errors.

---

### Task 3: Add `CursorScreenPosition` import where needed and remove Global impl

**Files:**
- Modify: `src/line.rs`

- [ ] **Remove `impl Global for CursorScreenPosition {}`**

Find and remove line 26 in `line.rs`:
```rust
impl Global for CursorScreenPosition {}
```

The `CursorScreenPosition` struct stays, but it no longer implements Global.

- [ ] **Build**

```bash
cargo build 2>&1 | head -30
```

Expected: build errors from any remaining code that reads/writes `CursorScreenPosition` as a global. Fix them.

---

### Task 4: Refactor title_bar to accept FileInfo directly

**Files:**
- Modify: `src/title_bar.rs`

- [ ] **Remove `impl Global for FileInfo {}`**

Find and remove the line `impl Global for FileInfo {}` in `title_bar.rs`.

- [ ] **Change title_bar function signature**

Change:
```rust
pub fn title_bar(theme: &EditorTheme, cx: &mut App) -> impl IntoElement {
    let file_info = FileInfo::global(cx);
```

To:
```rust
pub fn title_bar(theme: &EditorTheme, file_info: &FileInfo, cx: &mut App) -> impl IntoElement {
```

(The local `file_info` variable is no longer needed since it's now a parameter.)

- [ ] **Simplify the function body**

Remove `let file_info = FileInfo::global(cx);` line (already handled by parameter).

Clean up unused imports: remove `Global`, `ReadGlobal` from the `use gpui::{...}` line if they're no longer needed elsewhere.

- [ ] **Build**

```bash
cargo build 2>&1 | head -30
```

Expected: Build errors from callers of `title_bar`. That's OK — we'll fix callers in a later task.

---

### Task 5: Refactor status_bar to accept StatusBarInfo directly

**Files:**
- Modify: `src/status_bar.rs`

- [ ] **Remove `impl Global for StatusBarInfo {}`**

Find and remove `impl Global for StatusBarInfo {}`.

- [ ] **Change status_bar function signature and body**

Change:
```rust
pub fn status_bar(cx: &App) -> impl IntoElement {
    let info = StatusBarInfo::global(cx);
    let theme = EditorTheme::global(cx);
    let config = Config::global(cx);
```

To:
```rust
pub fn status_bar(info: &StatusBarInfo, theme: &EditorTheme, config: &Config) -> impl IntoElement {
```

Remove the three global reads. Remove unused imports (`Global`, `ReadGlobal`, `App`).

- [ ] **Update tests**

Change the test module to construct `StatusBarInfo` directly instead of reading from globals. The test at line 168-409 reads `build_context_display` which is a pure function and doesn't use globals, so no test changes needed for that.

- [ ] **Build**

```bash
cargo build 2>&1 | head -30
```

Expected: Build errors from callers of `status_bar`. That's OK.

---

### Task 6: Refactor WindowShadow to be purely decorative

**Files:**
- Modify: `src/window.rs`

- [ ] **Remove title_bar and status_bar from WindowShadow::render**

In `window.rs`, `RenderOnce for WindowShadow`, replace:
```rust
                    .child(title_bar(theme, cx))
                    .child(
                        // Content area
                        div().flex_1().min_h_0().w_full().children(self.children),
                    )
                    .child(status_bar(cx)),
```

With:
```rust
                    .child(
                        // Content area (RootView controls title_bar + editor + status_bar)
                        div().flex_1().min_h_0().w_full().children(self.children),
                    ),
```

Remove unused imports: `use crate::{editor::EditorTheme, status_bar::status_bar, title_bar::title_bar};` → `use crate::editor::EditorTheme;`

- [ ] **Build**

```bash
cargo build 2>&1 | head -30
```

---

### Task 7: Update RootView to control full layout

**Files:**
- Modify: `src/main.rs`

- [ ] **Add FileInfo and StatusBarInfo fields to RootView**

Change `RootView` struct:
```rust
struct RootView {
    editor: Entity<Editor>,
}
```

To:
```rust
struct RootView {
    editor: Entity<Editor>,
    file_info: FileInfo,
}
```

(The `StatusBarInfo` will be read directly from the Editor, so we don't need to store it on RootView.)

Add necessary imports:
```rust
use rustmd::status_bar::{status_bar, StatusBarInfo};
use rustmd::title_bar::{title_bar, FileInfo};
```

- [ ] **Rewrite RootView::render to control full layout**

Replace the current `RootView::render` (lines 112-179):

```rust
impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = EditorTheme::global(cx).clone();
        let config = Config::global(cx);

        // Read editor state for title bar and status bar
        let editor = self.editor.read(cx);
        self.file_info.path = editor.file_path().cloned();
        self.file_info.dirty = editor.is_dirty();
        let status_info = editor.status_info().clone();
        drop(editor);

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
                    .flex()
                    .flex_col()
                    .child(title_bar(theme, &self.file_info, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .child(EditorImeElement::new(self.editor.clone())),
                    )
                    .child(status_bar(&status_info, &theme, &config)),
            )
    }
}
```

- [ ] **Update the RootView construction**

In the window creation (line 101), update the `RootView` creation:
```rust
                cx.new(|_cx| RootView {
                    editor,
                    file_info: FileInfo {
                        path: initial_path.clone(),
                        dirty: false,
                    },
                })
```

- [ ] **Remove global initialization for removed types**

Remove from the init code (lines 39-46):
```rust
        cx.set_global(CursorScreenPosition::default());
```
And:
```rust
        cx.set_global(FileInfo {
            path: initial_path.clone(),
            dirty: false,
        });
```
And:
```rust
        cx.set_global(StatusBarInfo::default());
```

Keep:
```rust
        cx.set_global(config);
        cx.set_global(KeyMode::default());
```

The `Config` global is still needed (for status_bar). The `EditorTheme` is set up elsewhere (in user_config flow).

- [ ] **Build**

```bash
cargo build 2>&1 | head -40
```

Fix any compilation errors (likely imports and type mismatches).

---

### Task 8: Fix Editor's FileInfo global writes — remove them

**Files:**
- Modify: `src/editor/mod.rs`

- [ ] **Remove `is_primary`-guarded FileInfo global write**

In Editor's `render` (lines 3876-3885), remove the entire block:
```rust
        if self.is_primary {
            let file_info = FileInfo::global(cx);
            let dirty = self.state.buffer.is_dirty();
            if file_info.path != self.file_path || file_info.dirty != dirty {
                cx.set_global(FileInfo {
                    path: self.file_path.clone(),
                    dirty,
                });
            }
        }
```

Since `RootView` now reads `file_path()` and `is_dirty()` directly from the Editor, this global write is unnecessary.

- [ ] **Remove FileInfo writes from `save`, `save_as`, `open_file`, `new_file`**

In `save` (around line 3629), remove:
```rust
        cx.set_global(FileInfo {
            path: self.file_path.clone(),
            dirty: false,
        });
```

In `save_as` (around line 3662), remove:
```rust
        cx.set_global(FileInfo {
            path: self.file_path.clone(),
            dirty: false,
        });
```

In `open_file` (around line 3700), remove:
```rust
        cx.set_global(FileInfo {
            path: self.file_path.clone(),
            dirty: false,
        });
```

In `new_file` (around line 3723), remove:
```rust
        cx.set_global(FileInfo {
            path: None,
            dirty: false,
        });
```

- [ ] **Remove unused imports**

Check and remove `use crate::title_bar::FileInfo;` from editor/mod.rs if no longer used.

- [ ] **Fix file_ops.rs**

In `src/file_ops.rs`, remove or comment out `update_file_info_global` and `update_file_info_from_editor` functions since they're no longer needed. Check for callers first:

```bash
rg "update_file_info_global\|update_file_info_from_editor" src/
```

If no callers, remove the functions. If there are callers, update them.

- [ ] **Build**

```bash
cargo build 2>&1 | head -40
```

---

### Task 9: Fix Editor async code to use stored window_handle

**Files:**
- Modify: `src/editor/mod.rs`

- [ ] **Replace `cx.windows().first()` with stored handle**

There are 4 occurrences at lines 2055, 2094, 2161, 2266. Each follows the pattern:
```rust
let window = cx.windows().first().cloned();
```

Replace each with:
```rust
let window = self.window_handle.clone();
```

**Important**: `cx.windows()` returns `Vec<AnyWindowHandle>` and `.first().cloned()` gives `Option<AnyWindowHandle>`. The stored `self.window_handle` is also `Option<AnyWindowHandle>`, so the types match.

- [ ] **Build**

```bash
cargo build 2>&1 | head -30
```

---

### Task 10: Add NewWindow action and window factory

**Files:**
- Modify: `src/window.rs`
- Modify: `src/main.rs`

- [ ] **Define NewWindow action**

In `src/window.rs`, line 9, add `NewWindow`:
```rust
actions!(window, [CloseWindow, Quit, MinimizeWindow, ZoomWindow, NewWindow]);
```

- [ ] **Export NewWindow**

Ensure `NewWindow` is re-exported. In `src/main.rs`, update the import:
```rust
use rustmd::window::{window_shadow, CloseWindow, MinimizeWindow, ZoomWindow, NewWindow};
```

- [ ] **Create window factory function**

In `src/main.rs`, create a reusable function before `RootView`:

```rust
fn open_new_window(user_cfg: &UserConfig, cx: &mut App) {
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
            })
        },
    )
    .unwrap();
}
```

- [ ] **Wire up NewWindow action in RootView**

In `RootView::render`, add before the `.flex()` chain:
```rust
                    .on_action(cx.listener(|_: &mut RootView, _: &NewWindow, window, cx| {
                        let user_cfg = UserConfig::load();
                        open_new_window(&user_cfg, cx);
                    }))
```

- [ ] **Build**

```bash
cargo build 2>&1 | head -30
```

Fix any import issues (add `use crate::user_config::UserConfig;`, etc.)

---

### Task 11: Add keyboard shortcut for NewWindow

**Files:**
- Modify: `src/main.rs`

- [ ] **Add key binding**

In `main.rs`, in the `cx.bind_keys([...])` call (line 48), add:
```rust
            KeyBinding::new("ctrl-shift-n", NewWindow, None),
```

- [ ] **Build**

```bash
cargo build 2>&1 | head -30
```

---

### Task 12: Refactor toolbar to icon-based and add NewWindow button

**Files:**
- Modify: `src/menu.rs`

- [ ] **Update toolbar buttons with icons**

Replace `get_toolbar_buttons` function:

```rust
pub fn get_toolbar_buttons(cx: &App) -> Vec<ToolbarButton> {
    let mode_text = if KeyMode::is_mac(cx) {
        "Mac"
    } else {
        "Win"
    };

    vec![
        ToolbarButton::new("📄", NewFile),
        ToolbarButton::new("📂", OpenFile),
        ToolbarButton::new("💾", Save),
        ToolbarButton::new("🔲", NewWindow),
        ToolbarButton::new(format!("⌨ {mode_text}"), ToggleKeyMode),
    ]
}
```

- [ ] **Add `NewWindow` action to menu imports**

Add `NewWindow` to the import line:
```rust
use crate::window::{CloseWindow, MinimizeWindow, ZoomWindow, NewWindow};
```

Wait — `menu.rs` imports from `file_ops.rs` and `key_mode.rs` currently. Let me check what's imported and add accordingly.

Actually, `NewWindow` is defined in `window.rs`. Let me check what `menu.rs` imports:

```rust
use crate::editor::EditorTheme;
use crate::file_ops::{NewFile, OpenFile, Save, SaveAs};
use crate::key_mode::KeyMode;
```

Add:
```rust
use crate::window::NewWindow;
```

- [ ] **Build**

```bash
cargo build 2>&1 | head -30
```

---

### Task 13: Fix remaining compilation issues

**Files:**
- All modified files

- [ ] **Full build**

```bash
cargo build 2>&1
```

Fix any remaining errors:
1. Missing imports anywhere globals were removed
2. `status_bar()` call signature in RootView (now needs `info`, `theme`, `cx`)
3. `title_bar()` call signature (now needs `file_info`)
4. `cx.refresh_windows()` — retains existing behavior
5. `window_shadow` may need import adjustments

- [ ] **Run existing tests**

```bash
cargo test 2>&1
```

Fix any test failures.

- [ ] **Commit the working multi-window implementation**

```bash
git add -A
git commit -m "feat: multi-window support + icon toolbar

- Add NewWindow action, keyboard shortcut (Ctrl+Shift+N), and toolbar button
- Remove per-window globals (FileInfo, StatusBarInfo, CursorScreenPosition)
- Refactor title_bar and status_bar to accept data directly
- RootView controls full layout (title_bar + editor + status_bar)
- Icon-based toolbar for compactness
- Editor stores its own window_handle (replaces cx.windows().first())
- CursorScreenPosition shared via Rc<RefCell> between Editor and Line"
```

---

## Build & Test Commands

```bash
# Full build
cargo build 2>&1

# Run tests
cargo test 2>&1

# Run specific test file
cargo test -p rustmd status_bar 2>&1
```
