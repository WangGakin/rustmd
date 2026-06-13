use gpui::{App, Global, SharedString};

#[derive(Clone)]
pub struct Tooltip {
    pub text: Option<SharedString>,
}

impl Global for Tooltip {}

impl Tooltip {
    pub fn show(text: impl Into<SharedString>, cx: &mut App) {
        cx.set_global(Self {
            text: Some(text.into()),
        });
    }

    pub fn hide(cx: &mut App) {
        cx.set_global(Self { text: None });
    }
}
