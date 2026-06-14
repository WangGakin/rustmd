use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use gpui::{Rgba, rgb};
use serde::{Deserialize, Serialize};

use crate::editor::EditorTheme;

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

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct SerializedTheme {
    pub background: String,
    pub foreground: String,
    pub selection: String,
    pub comment: String,
    pub red: String,
    pub orange: String,
    pub yellow: String,
    pub green: String,
    pub cyan: String,
    pub purple: String,
    pub pink: String,
}

pub enum Preset {
    Dracula,
    Nord,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            theme: SerializedTheme::from_preset(&Preset::Dracula),
            text_font: default_text_font(),
            code_font: default_code_font(),
            font_size_rem: 0.875,
            recent_files: Vec::new(),
        }
    }
}

impl SerializedTheme {
    pub fn from_preset(preset: &Preset) -> Self {
        match preset {
            Preset::Nord => Self::nord(),
            Preset::Dracula => Self::dracula(),
        }
    }

    fn dracula() -> Self {
        Self {
            background: "#282A36".into(),
            foreground: "#F8F8F2".into(),
            selection: "#44475A".into(),
            comment: "#6272A4".into(),
            red: "#FF5555".into(),
            orange: "#FFB86C".into(),
            yellow: "#F1FA8C".into(),
            green: "#50FA7B".into(),
            cyan: "#8BE9FD".into(),
            purple: "#BD93F9".into(),
            pink: "#FF79C6".into(),
        }
    }

    fn nord() -> Self {
        Self {
            background: "#2E3440".into(),
            foreground: "#D8DEE9".into(),
            selection: "#434C5E".into(),
            comment: "#616E88".into(),
            red: "#BF616A".into(),
            orange: "#D08770".into(),
            yellow: "#EBCB8B".into(),
            green: "#A3BE8C".into(),
            cyan: "#88C0D0".into(),
            purple: "#B48EAD".into(),
            pink: "#BF88BC".into(),
        }
    }

    pub fn to_editor_theme(&self) -> EditorTheme {
        EditorTheme {
            background: parse_hex(&self.background),
            foreground: parse_hex(&self.foreground),
            selection: parse_hex(&self.selection),
            comment: parse_hex(&self.comment),
            red: parse_hex(&self.red),
            orange: parse_hex(&self.orange),
            yellow: parse_hex(&self.yellow),
            green: parse_hex(&self.green),
            cyan: parse_hex(&self.cyan),
            purple: parse_hex(&self.purple),
            pink: parse_hex(&self.pink),
        }
    }

    pub fn from_editor_theme(theme: &EditorTheme) -> Self {
        Self {
            background: to_hex(theme.background),
            foreground: to_hex(theme.foreground),
            selection: to_hex(theme.selection),
            comment: to_hex(theme.comment),
            red: to_hex(theme.red),
            orange: to_hex(theme.orange),
            yellow: to_hex(theme.yellow),
            green: to_hex(theme.green),
            cyan: to_hex(theme.cyan),
            purple: to_hex(theme.purple),
            pink: to_hex(theme.pink),
        }
    }
}

fn parse_hex(s: &str) -> Rgba {
    let s = s.trim_start_matches('#');
    let hex = u32::from_str_radix(s, 16).unwrap_or(0xFFFFFF);
    rgb(hex)
}

fn to_hex(c: Rgba) -> String {
    // Rgba uses u8 components via rgb(), extract RGB as hex
    // GPUI's rgb() packs as 0xRRGGBB. Since we only use opaque colors, no alpha.
    let r = (c.r * 255.0) as u32;
    let g = (c.g * 255.0) as u32;
    let b = (c.b * 255.0) as u32;
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rustmd")
        .join("config.json")
}

pub fn load_config() -> UserConfig {
    let path = config_path();
    #[cfg(debug_assertions)]
    eprintln!("[rustmd] config: {:?}", path);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => {
            let cfg = UserConfig::default();
            save_config(&cfg);
            cfg
        }
    }
}

pub fn save_config(cfg: &UserConfig) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(&path, json);
    }
}

static RECENT_FILES: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| {
    Mutex::new(load_config().recent_files)
});

pub fn add_recent_file(path: &Path) {
    let path_str = path.to_string_lossy().to_string();
    if path_str.is_empty() {
        return;
    }
    let files = {
        let mut files = RECENT_FILES.lock().unwrap();
        files.retain(|f| f != &path_str);
        files.insert(0, path_str);
        files.truncate(5);
        files.clone()
    };
    let mut cfg = load_config();
    cfg.recent_files = files;
    save_config(&cfg);
}

pub fn clear_recent_files() {
    {
        let mut files = RECENT_FILES.lock().unwrap();
        files.clear();
    }
    let mut cfg = load_config();
    cfg.recent_files.clear();
    save_config(&cfg);
}

pub fn recent_files() -> Vec<String> {
    RECENT_FILES.lock().unwrap().clone()
}

fn default_font_size() -> f32 {
    0.875
}

fn default_text_font() -> String {
    #[cfg(target_os = "windows")]
    { "Segoe UI".into() }
    #[cfg(target_os = "macos")]
    { ".AppleSystemUIFont".into() }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { "Liberation Sans".into() }
}

fn default_code_font() -> String {
    #[cfg(target_os = "windows")]
    { "Consolas".into() }
    #[cfg(target_os = "macos")]
    { "Menlo".into() }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { "Liberation Mono".into() }
}
