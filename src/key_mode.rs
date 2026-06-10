use gpui::{App, Global, ReadGlobal};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum KeyMode {
    Win,
    Mac,
}

impl Global for KeyMode {}

impl Default for KeyMode {
    fn default() -> Self {
        Self::Win
    }
}

impl KeyMode {
    pub fn is_mac(cx: &App) -> bool {
        *Self::global(cx) == Self::Mac
    }

    pub fn toggle(cx: &mut App) {
        let current = *Self::global(cx);
        let new_mode = match current {
            Self::Win => Self::Mac,
            Self::Mac => Self::Win,
        };
        cx.set_global(new_mode);
    }
}
