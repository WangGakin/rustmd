# Recent Files (历史文件) — Design Spec

## Overview

Add a "recent files" feature to rustmd that tracks the last 5 opened files and provides a dropdown menu in the title bar for quick re-opening.

## Data Model

Extend `UserConfig` in `src/user_config.rs`:

```rust
#[derive(Serialize, Deserialize)]
pub struct UserConfig {
    pub theme: SerializedTheme,
    pub text_font: String,
    pub code_font: String,
    pub font_size_rem: f32,
    #[serde(default)]
    pub recent_files: Vec<String>,  // max 5, sorted most-recent-first
}
```

`#[serde(default)]` ensures existing configs without the field deserialize to an empty vec.

## Persistence

Reuse `save_config()` which already writes to `%APPDATA%/rustmd/config.json` (or equivalent on other platforms). Two new public functions:

**`add_recent_file(path: &Path)`:**
1. Convert path to string, skip if empty
2. Remove any existing entry matching the same path
3. Insert at index 0
4. Truncate to 5 elements
5. Save config via `save_config()`

**`clear_recent_files()`:**
1. Set `recent_files` to empty vec
2. Save config via `save_config()`

## Trigger Points — Record a File

Record the file path when the user successfully opens or saves a file:

**`Editor::open_file()`** (line 3150): After successfully reading the file and setting `self.file_path`, call `add_recent_file(&path)`.

**`Editor::save_as()`** (line 3121): After successfully writing and setting `self.file_path`, call `add_recent_file(&path)`.

**`main.rs` startup** (line 88-93): After creating the editor with `--file <path>`, call `add_recent_file(&path)`.

## New Actions

In `src/file_ops.rs`, add:

```rust
#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct OpenRecentFile(pub usize);

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct ClearRecentFiles;
```

`OpenRecentFile(usize)` carries an index into the recent files list. This explicit struct pattern (same as `DispatchEditorAction`) is required because GPUI's `actions!` macro only generates parameterless actions.

## Refactor: Extract `open_file_at()`

Extract a method from `open_file()` that takes a `PathBuf` directly (no dialog):

```rust
pub fn open_file_at(&mut self, path: PathBuf, cx: &mut Context<Self>) {
    // same logic as open_file() from line 3151-3180,
    // but using the provided path instead of pick_open_file()
}
```

`open_file()` calls `pick_open_file()` then delegates to `open_file_at()`.

This is used by both the file dialog path and the recent-files path.

## UI — Title Bar Dropdown

### Location

In `src/title_bar.rs`, right side of the title bar: between the centered file-name area and the traffic-light buttons.

### Component

A GPUI dropdown button style, before the traffic light div:

```
. 🕐 ▼  |  minimize max close
```

**Button states:**
- Recent files not empty: clickable, opens dropdown
- Recent files empty: disabled/greyed out

### Dropdown Popover

Similar pattern to the existing About popover (`ToggleAbout` in `main.rs`):

1. A `ToggleRecentFiles` action toggles the dropdown open/closed
2. When open, a transparent overlay catches outside clicks to close
3. The dropdown renders positioned below the button

**Dropdown contents (top to bottom):**
- For each of up to 5 recent files, a clickable row:
  - Text: `"{filename} — {parent_dir_name}"` (e.g., `"notes.md — Desktop"`)
  - If file no longer exists on disk, render with dimmed opacity but still clickable
  - On click: dispatch `OpenRecentFile(index)`
- A separator line
- "Clear Recent Files" button → dispatches `ClearRecentFiles`

### Styling

Use the same theme colors as the toolbar (`.text_color(theme.foreground)`, `.hover(|s| s.bg(theme.selection))`).

## FileInfo Changes

Add `recent_files: Vec<String>` to `FileInfo` in `title_bar.rs` so the title bar has access to the list.

In `main.rs` `RootView::render()`, populate from `UserConfig::global()` or pass via editor.

## Event Handling

**`OpenRecentFile(i)` handler:**
1. Read `UserConfig` to get the path at index `i`
2. If current buffer is dirty → show `confirm_discard()` dialog
3. Call `editor.open_file_at(path, cx)`

**`ClearRecentFiles` handler:**
1. Call `clear_recent_files()`
2. Update `FileInfo.recent_files` to empty
3. Close dropdown via `ToggleRecentFiles`

## Files Changed

| File | Change |
|------|--------|
| `src/user_config.rs` | Add `recent_files` field, `add_recent_file()`, `clear_recent_files()` |
| `src/file_ops.rs` | Add `OpenRecentFile`, `ClearRecentFiles` actions |
| `src/editor/mod.rs` | Add `open_file_at()`, record in `open_file()` and `save_as()` |
| `src/title_bar.rs` | Add recent-files button and dropdown popover |
| `src/main.rs` | Bind `OpenRecentFile`/`ClearRecentFiles` actions, pass recent files to title bar, record on `--file` startup |

## Rationale

- **Extending UserConfig** over a separate file avoids adding another I/O pathway
- **Reusing `save_config()`** keeps persistence consistent — it's already called on config dir creation
- **Max 5 items** keeps the dropdown short and avoids scrolling
- **`filename — parent` format** is informative at a glance without taking excessive horizontal space
- **Dropdown over toolbar button** keeps the toolbar uncluttered and matches the "right side" placement the user chose
