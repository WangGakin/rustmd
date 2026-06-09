use gpui::*;

const PADDING: Pixels = px(16.0);

pub struct MarkdownPreview {
    content: SharedString,
}

impl MarkdownPreview {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            content: "## Preview\n\nMarkdown preview will render here.".into(),
        }
    }

    #[allow(dead_code)]
    pub fn update_content(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content = format!("## Preview\n\nRendering:\n\n{}", text).into();
        cx.notify();
    }
}

impl Render for MarkdownPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .p(PADDING)
            .text_size(px(14.0))
            .text_color(rgb(0xa6adc8))
            .child(self.content.clone())
    }
}
