use std::path::{Path, PathBuf};

use gpui::*;
use gpui::prelude::*;

use crate::editor::EditorTheme;

actions!(file_explorer, [ToggleFileExplorer]);

/// Number of files shown per page.
pub const PAGE_SIZE: usize = 18;

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct OpenExplorerFile {
    pub path: PathBuf,
    /// Whether Shift was held during click — opens in same window instead of new.
    pub shift: bool,
}

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct ExplorerPrevPage;

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct ExplorerNextPage;

/// Scan `folder` for `.md` and `.txt` files, returning them sorted
/// case-insensitively by filename.
pub fn scan_folder(folder: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(folder) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .map(|ext| ext == "md" || ext == "txt")
                        .unwrap_or(false)
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort_by(|a, b| {
        let a_name = a
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase());
        let b_name = b
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase());
        a_name.cmp(&b_name)
    });
    files
}

/// Render the file explorer sidebar overlay with pagination.
pub fn file_explorer_panel(
    folder: &Path,
    files: &[PathBuf],
    current_file: Option<&PathBuf>,
    page: usize,
    theme: &EditorTheme,
) -> impl IntoElement {
    let folder_display = folder.display().to_string();
    let total_pages = if files.is_empty() {
        0
    } else {
        files.len().div_ceil(PAGE_SIZE)
    };
    let start = page * PAGE_SIZE;
    let page_files = &files[start..(start + PAGE_SIZE).min(files.len())];
    let has_prev = page > 0;
    let has_next = page + 1 < total_pages;

    div()
        .absolute()
        .top(rems(2.5))
        .left(rems(0.5))
        .w(rems(16.0))
        .occlude()
        .bg(theme.background)
        .border_1()
        .border_color(theme.comment)
        .rounded(px(4.0))
        .flex()
        .flex_col()
        // Folder header
        .child(
            div()
                .px(rems(0.75))
                .py(rems(0.4))
                .text_xs()
                .text_color(theme.comment)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(folder_display),
        )
        .child(
            div()
                .mx(rems(0.75))
                .border_t_1()
                .border_color(theme.selection),
        )
        // Prev page button
        .when(has_prev, |this| {
            let action = ExplorerPrevPage.boxed_clone();
            this.child(
                div()
                    .px(rems(0.75))
                    .py(rems(0.2))
                    .text_sm()
                    .text_color(theme.comment)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.selection))
                    .child("\u{25B2}  Prev")
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        window.dispatch_action(action.boxed_clone(), cx);
                    }),
            )
        })
        // Empty state
        .when(files.is_empty(), |this| {
            this.child(
                div()
                    .px(rems(0.75))
                    .py(rems(0.6))
                    .text_color(theme.comment)
                    .text_sm()
                    .child("No .md or .txt files found"),
            )
        })
        // File list (current page)
        .children(page_files.iter().map(|path| {
            let is_active = current_file.is_some_and(|cf| cf == path);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            let path_for_click = path.clone();
            div()
                .px(rems(0.75))
                .py(rems(0.3))
                .text_sm()
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .text_color(if is_active { theme.cyan } else { theme.foreground })
                .when(is_active, |this| this.bg(theme.selection))
                .cursor_pointer()
                .hover(|s| {
                    if is_active {
                        s
                    } else {
                        s.bg(theme.selection)
                    }
                })
                .child(name)
                .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, window, cx| {
                    window.dispatch_action(
                        Box::new(OpenExplorerFile {
                            path: path_for_click.clone(),
                            shift: event.modifiers.shift,
                        }),
                        cx,
                    );
                })
                .into_any_element()
        }))
        // Next page button
        .when(has_next, |this| {
            let action = ExplorerNextPage.boxed_clone();
            this.child(
                div()
                    .px(rems(0.75))
                    .py(rems(0.2))
                    .text_sm()
                    .text_color(theme.comment)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.selection))
                    .child("Next  \u{25BC}")
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        window.dispatch_action(action.boxed_clone(), cx);
                    }),
            )
        })
        // Page indicator (shown when multiple pages)
        .when(total_pages > 1, |this| {
            this.child(
                div()
                    .px(rems(0.75))
                    .py(rems(0.2))
                    .text_xs()
                    .text_color(theme.comment)
                    .child(format!("{}/{}", page + 1, total_pages)),
            )
        })
}
