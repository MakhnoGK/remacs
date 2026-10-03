mod display_line;
mod editor_view;
mod key_input;

use std::path::PathBuf;
use std::process::ExitCode;

use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, prelude::*, px,
    size,
};
use remacs_core::{Editor, Plugin};
use remacs_editing::EditingPlugin;
use remacs_languages::LanguagesPlugin;

use editor_view::EditorView;

const BUILTIN_PLUGINS: &[&dyn Plugin] = &[&EditingPlugin, &LanguagesPlugin];

fn main() -> ExitCode {
    let mut editor = Editor::new(BUILTIN_PLUGINS);
    if let Some(path) = std::env::args_os().nth(1).map(PathBuf::from)
        && let Err(error) = editor.visit_file(&path)
    {
        eprintln!("remacs: {}: {error}", path.display());
        return ExitCode::FAILURE;
    }

    Application::new().run(move |cx: &mut App| {
        cx.on_window_closed(|cx| cx.quit()).detach();
        let bounds = Bounds::centered(None, size(px(1440.0), px(900.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: None,
                appears_transparent: true,
                traffic_light_position: Some(point(px(18.0), px(16.0))),
            }),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            let view = cx.new(|cx| EditorView::new(editor, cx));
            window.focus(view.read(cx).focus_handle());
            view
        })
        .expect("failed to open window");
        cx.activate(true);
    });
    ExitCode::SUCCESS
}
