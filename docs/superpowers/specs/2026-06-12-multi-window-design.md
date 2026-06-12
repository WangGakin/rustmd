# Multi-Window Support for RustMD

## Objective

Enable multiple independent OS windows in RustMD, allowing users to:
- Open two documents side by side for comparison
- Create a new document in a new window while keeping existing documents open

## Requirements

1. **Independent OS windows**: Each window is a separate OS window with its own document
2. **Trigger methods**:
   - Toolbar button "New Window" (icon-based)
   - Keyboard shortcut `Ctrl+Shift+N`
3. **Each window fully independent**: file path, dirty state, cursor position, scroll position, status bar — all per-window
4. **Close behavior**: Closing the last window quits the app (already implemented in `cx.on_window_closed`)
5. **Toolbar redesign**: Replace current text-only toolbar with an icon-based toolbar for compactness and future extensibility

## Architecture

### Before (Single-Window)

```
Global state:
  FileInfo         (Global trait)
  StatusBarInfo    (Global trait)
  CursorScreenPosition (Global trait)

WindowShadow renders:
  - title_bar(theme, cx)          ← reads FileInfo::global()
  - editor content (from RootView)
  - status_bar(cx)                ← reads StatusBarInfo::global()
```

### After (Multi-Window)

```
Persisted global state (shared):
  Config, EditorTheme, KeyMode

Per-window state (on RootView or Editor):
  file_info: FileInfo
  status_info: StatusBarInfo
  cursor_screen_pos: CursorScreenPosition

RootView renders full layout:
  window_shadow(theme)             ← decorations only
    title_bar(theme, &file_info, cx)
    EditorImeElement(editor)
    status_bar(theme, &status_info, cx)
```

## Changes by File

### 1. `src/window.rs` — Add NewWindow, strip layout from WindowShadow

- Add `NewWindow` to `actions!(window, [...])`
- Remove `title_bar()` and `status_bar()` calls from `WindowShadow::render`
- `WindowShadow` becomes purely decorative (drop shadow, resize handles, rounded corners, border)

### 2. `src/title_bar.rs` — Accept FileInfo directly

- Change `pub fn title_bar(theme, cx)` → `pub fn title_bar(theme, file_info: &FileInfo, cx)`
- Remove `FileInfo::global(cx)` call; use the passed `file_info` parameter
- `FileInfo` struct stays (move out of `title_bar.rs` or keep as-is, no longer `impl Global`)

### 3. `src/status_bar.rs` — Accept StatusBarInfo directly

- Change `pub fn status_bar(cx)` → `pub fn status_bar(theme: &EditorTheme, info: &StatusBarInfo, cx)`
- Remove `StatusBarInfo::global(cx)` and `EditorTheme::global(cx)` calls
- `StatusBarInfo` struct stays, no longer `impl Global`
- Tests need minimal update (construct StatusBarInfo directly)

### 4. `src/line.rs` — Remove CursorScreenPosition global

- Remove `impl Global for CursorScreenPosition`
- Change line render code to return or set cursor screen position via callback

### 5. `src/editor/mod.rs` — Remove global writes, fix window refs

- In `Editor::Render`: stop writing to `FileInfo::global()`, `StatusBarInfo::global()`, `CursorScreenPosition::global()`
- Instead, store this data on fields that RootView can read, OR use a callback/event to notify RootView
- Fix `cx.windows().first()` references — use a stored window handle instead

### 6. `src/main.rs` — RootView controls layout, window factory

- `RootView::render` becomes the layout controller:
  - Renders `window_shadow(theme)` as outer container
  - Inside: `title_bar` → editor → `status_bar`
- Extract window creation into `fn open_new_window(cx: &mut App)` or `fn open_new_window(content: &str, cx: &mut App)`
- Bind `ctrl-shift-n` to `NewWindow`
- Remove `cx.set_global()` for `FileInfo`, `StatusBarInfo`, `CursorScreenPosition`
- Update `on_window_closed` to still quit when `cx.windows().is_empty()`
- Update `RootView` struct to hold `FileInfo` and `StatusBarInfo` data

### 7. `src/menu.rs` — Icon-based toolbar

- Replace text buttons with icon representation:
  - New: 📄 (Unicode U+1F4C4) or simple SVG/character
  - Open: 📂 (U+1F4C2)
  - Save: 💾 (U+1F4BE)
  - Save As: 💾+ (custom text)
  - New Window: 🔲 (U+1F532)
  - Key Mode: textual "Win"/"Mac" with keyboard icon
  - Group with visual separators (`|`)
- `ToolbarButton` struct may be extended with an optional `icon` field

### 8. `src/lib.rs` — Update exports if needed

- Remove or update any public re-exports of removed globals

### 9. `src/file_ops.rs` — Minor adjustments

- `update_file_info_global` and `update_file_info_from_editor` may be simplified or removed
- The `DIALOG_OPEN` atomic stays (already cross-window safe)

## Editor Data Flow

The key challenge: `Editor::Render` currently writes to globals. With multi-window, each `RootView` needs to know its editor's state.

**Approach**: `RootView` holds `file_info: FileInfo` and `status_info: StatusBarInfo` as fields. Before rendering, `RootView` reads from its `Editor` entity:

```rust
impl RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        self.file_info.path = editor.file_path().cloned();
        self.file_info.dirty = editor.is_dirty();
        self.status_info = editor.status_bar_info();  // new method on Editor
        
        // ... render layout using self.file_info and self.status_info
    }
}
```

The `Editor` already calculates all this data during its own `Render` (for line rendering, etc.) but was writing it to globals. With this design, `RootView` reads it directly from the Editor entity before rendering the chrome.

Alternatively, the Editor could store its status info as a plain field that RootView reads via `editor.read(cx).status_info`.

## Window Factory

```rust
fn open_new_window(user_cfg: &UserConfig, cx: &mut App) {
    let editor_config = EditorConfig { ... };
    cx.open_window(WindowOptions { ... }, |window, cx| {
        let editor = cx.new(|cx| Editor::with_config("", editor_config, cx));
        editor.update(cx, |e, cx| e.start_cursor_blink(window.window_handle(), cx));
        window.focus(&editor.read(cx).focus_handle(cx));
        cx.new(|_cx| RootView { editor, file_info: FileInfo::default(), status_info: StatusBarInfo::default() })
    }).unwrap();
}
```

## Icon Toolbar Design

Using Unicode symbols for zero-dependency icons:

| Button | Icon | Unicode |
|--------|------|---------|
| New | 📄 | U+1F4C4 |
| Open | 📂 | U+1F4C2 |
| Save | 💾 | U+1F4BE |
| New Window | 🔲 | U+1F532 |
| Key Mode | ⌨ | U+2328 + text |
| About | ⓘ | U+24D8 |

Render with separators between groups:

```
📄 📂 💾 | 🔲 | ⌨ Win | ⓘ
```

Buttons remain `ToolbarButton` with an added `icon` field. Text used for tooltip or accessibility.

## Migration Strategy

1. Make backup git commit (done: `27184cf`)
2. Refactor `title_bar.rs` and `status_bar.rs` to accept data directly (no global reads)
3. Refactor `WindowShadow` to remove title_bar/status_bar rendering
4. Update `RootView` to control layout
5. Add `NewWindow` action + factory function
6. Add keyboard shortcut + toolbar button
7. Update `Editor` render to not write globals
8. Remove global `FileInfo`, `StatusBarInfo`, `CursorScreenPosition`
9. Fix `Editor::cx.windows().first()` references
10. Update toolbar to icon-based
11. Build and test

## Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| `cx.windows().first()` assumes single window | Replace with stored window handle in Editor |
| File dialogs block on Windows (`rfd` nested loop) | Already handled via `window.defer()` + `DIALOG_OPEN` atomic |
| IME per-window state | EditorImeElement already per-editor; each window gets its own |
| Unicode icons render differently across platforms | Use simple ASCII fallback or test on target platform |
