use gpui::*;
use rustmd::config::Config;
use rustmd::editor::{Editor, EditorTheme};
use rustmd::editor::ime::{EditorImeElement, content_from_file};
use rustmd::line::CursorScreenPosition;
use rustmd::title_bar::FileInfo;
use rustmd::status_bar::StatusBarInfo;

fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    Application::new().run(|cx: &mut App| {
        cx.activate(true);

        let content = content_from_file("demo.md");
        let theme = EditorTheme::dracula();

        cx.set_global(theme.clone());
        cx.set_global(Config {
            file: None,
            demo: false,
            text_font: "Consolas".into(),
            code_font: "Consolas".into(),
            autosave: false,
            github_token: None,
            github_repo: None,
            agent: None,
        });
        cx.set_global(CursorScreenPosition::default());
        cx.set_global(FileInfo { path: "demo.md".into(), dirty: false });

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.set_global(StatusBarInfo::default());

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(900.0), px(700.0)),
                ))),
                ..Default::default()
            },
            |_window, cx| {
                let editor = cx.new(|cx| Editor::new(&content, cx));
                cx.new(|_cx| RootView { editor })
            },
        )
        .unwrap();
    });
}

struct RootView {
    editor: Entity<Editor>,
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0x282a36))
            .child(EditorImeElement::new(self.editor.clone()))
    }
}
