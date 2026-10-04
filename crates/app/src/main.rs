mod assets;
mod dialog_view;
mod display_line;
mod editor_view;
mod highlights;
mod key_input;
mod minibuffer_view;
mod popup_view;
mod preview_view;
mod table_view;
mod widgets;

use std::path::PathBuf;
use std::process::ExitCode;

use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, prelude::*, px,
    size,
};
use remacs_actions::ActionsPlugin;
use remacs_completion::CompletionPlugin;
use remacs_core::{Editor, Plugin};
use remacs_diagnostics::DiagnosticsPlugin;
use remacs_dired::{DiredPlugin, list_directory};
use remacs_documentation::DocumentationPlugin;
use remacs_editing::EditingPlugin;
use remacs_files::FilesPlugin;
use remacs_languages::{GRAMMARS, LanguagesPlugin, SERVERS};
use remacs_lsp::LspPlugin;
use remacs_minibuffer::MinibufferPlugin;
use remacs_syntax::SyntaxPlugin;
use remacs_xref::XrefPlugin;

use assets::Assets;
use editor_view::EditorView;

const BUILTIN_PLUGINS: &[&dyn Plugin] = &[
    &EditingPlugin,
    &MinibufferPlugin,
    &FilesPlugin,
    &DiredPlugin,
    &LanguagesPlugin,
    &SyntaxPlugin::new(GRAMMARS),
    &DiagnosticsPlugin,
    // Before their backends, so that what they ask after a command is sent in the same pass.
    &CompletionPlugin,
    &DocumentationPlugin,
    &XrefPlugin,
    &ActionsPlugin,
    &LspPlugin::new(SERVERS),
];

fn main() -> ExitCode {
    let mut editor = Editor::new(BUILTIN_PLUGINS);

    if let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) {
        let opened = if path.is_dir() {
            list_directory(&mut editor, &path).map_err(|error| error.0)
        } else {
            editor.visit_file(&path).map_err(|error| error.to_string())
        };
        if let Err(error) = opened {
            eprintln!("remacs: {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    }

    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
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
                let view = cx.new(|cx| EditorView::new(editor, window, cx));
                window.focus(view.read(cx).focus_handle());
                view
            })
            .expect("failed to open window");
            cx.activate(true);
        });
    ExitCode::SUCCESS
}
