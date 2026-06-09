mod editor;
mod markdown;
mod workspace;

use gpui::*;

fn main() {
    Application::new().run(|cx: &mut App| {
        cx.activate(true);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(100.), px(100.)),
                    size(px(1200.0), px(800.0)),
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("RustMD".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window: &mut Window, cx| {
                let workspace = cx.new(|cx| workspace::Workspace::new(cx));
                let editor = workspace.read(cx).editor.clone();
                let fh = editor.update(cx, |e, _| e.focus_handle_ref());
                window.focus(&fh);
                workspace
            },
        )
        .unwrap();
    });
}
