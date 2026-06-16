use std::path::PathBuf;

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{actions, Action};

use crate::config::Config;

actions!(file, [NewFile, OpenFile, Save, SaveAs]);

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct OpenRecentFile(pub usize);

#[derive(Clone, PartialEq, Debug, Action)]
#[action(no_json)]
pub struct ClearRecentFiles;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileOp {
    New,
    Open,
    Save,
    SaveAs,
}

static DIALOG_OPEN: AtomicBool = AtomicBool::new(false);

pub fn is_dialog_open() -> bool {
    DIALOG_OPEN.load(Ordering::SeqCst)
}

pub fn set_dialog_open(open: bool) {
    DIALOG_OPEN.store(open, Ordering::SeqCst);
}

pub fn file_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new()
        .add_filter("Markdown & Text", &["md", "txt"])
        .add_filter("All Files", &["*"])
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DiscardChoice {
    Save,
    DontSave,
    Cancel,
}

pub fn confirm_discard() -> DiscardChoice {
    let result = rfd::MessageDialog::new()
        .set_title("Unsaved Changes")
        .set_description("You have unsaved changes. Do you want to save before continuing?")
        .set_level(rfd::MessageLevel::Warning)
        .set_buttons(rfd::MessageButtons::YesNoCancel)
        .show();

    match result {
        rfd::MessageDialogResult::Yes => DiscardChoice::Save,
        rfd::MessageDialogResult::No => DiscardChoice::DontSave,
        _ => DiscardChoice::Cancel,
    }
}

pub fn pick_open_file() -> Option<PathBuf> {
    file_dialog().pick_file()
}

pub fn pick_save_file(default_name: Option<&str>) -> Option<PathBuf> {
    let mut dialog = file_dialog();
    if let Some(name) = default_name {
        dialog = dialog.set_file_name(name);
    }
    dialog.save_file()
}

pub fn initial_content(config: &Config) -> String {
    if let Some(ref path) = config.file {
        std::fs::read_to_string(path).unwrap_or_default()
    } else if config.demo {
        crate::editor::ime::content_from_file("demo.md")
    } else {
        String::new()
    }
}

pub fn initial_file_path(config: &Config) -> Option<PathBuf> {
    config.file.clone()
}


