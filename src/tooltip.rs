use gpui::{App, Global, Pixels, Point, SharedString};

#[derive(Clone)]
pub struct Tooltip {
    pub text: Option<SharedString>,
    pub position: Option<Point<Pixels>>,
}

impl Global for Tooltip {}

impl Tooltip {
    pub fn show(text: impl Into<SharedString>, position: Option<Point<Pixels>>, cx: &mut App) {
        cx.set_global(Self {
            text: Some(text.into()),
            position,
        });
    }

    pub fn hide(cx: &mut App) {
        cx.set_global(Self { text: None, position: None });
    }
}

