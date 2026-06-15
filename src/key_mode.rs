use gpui::{App, Global, ReadGlobal};

#[derive(Clone, Copy, PartialEq, Debug)]
#[derive(Default)]
pub enum KeyMode {
    Win,
    #[default]
    Mac,
}

impl Global for KeyMode {}


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
