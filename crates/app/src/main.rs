mod editor_view;

use std::path::PathBuf;
use std::process::ExitCode;

use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, prelude::*, px,
    size,
};
use remacs_text::Text;

use editor_view::EditorView;

fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: remacs <file>");
        return ExitCode::FAILURE;
    };
    let text = match Text::open(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("remacs: {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let title = format!("{file_name} — remacs");

    Application::new().run(move |cx: &mut App| {
        cx.on_window_closed(|cx| cx.quit()).detach();
        let bounds = Bounds::centered(None, size(px(1440.0), px(900.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(title.clone().into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(18.0), px(16.0))),
            }),
            ..Default::default()
        };
        cx.open_window(options, |_, cx| cx.new(|_| EditorView::new(text, title)))
            .expect("failed to open window");
        cx.activate(true);
    });
    ExitCode::SUCCESS
}
