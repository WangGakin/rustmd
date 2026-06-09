use gpui::*;

use crate::editor::Editor;
use crate::editor::EditorElement;
use crate::markdown::MarkdownPreview;

pub struct Workspace {
    pub editor: Entity<Editor>,
    preview: Entity<MarkdownPreview>,
}

impl Workspace {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| Editor::new(cx));
        let preview = cx.new(|cx| MarkdownPreview::new(cx));

        Self { editor, preview }
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0x181825))
            .text_color(rgb(0xcdd6f4))
            .flex()
            .flex_row()
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .border_r_1()
                    .border_color(rgb(0x313244))
                    .child(EditorElement::new(self.editor.clone())),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .child(self.preview.clone()),
            )
    }
}
