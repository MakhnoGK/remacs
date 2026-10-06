mod assets;
mod changes_view;
mod conflict_view;
mod dialog_view;
mod display_line;
mod editor_view;
mod highlights;
mod key_input;
mod minibuffer_view;
mod outline_view;
mod popup_view;
mod preview_view;
mod settings;
mod table_view;
mod widgets;

use std::path::PathBuf;
use std::process::ExitCode;

use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, point, prelude::*, px, size,
};
use remacs_actions::ActionsPlugin;
use remacs_completion::CompletionPlugin;
use remacs_core::{Editor, Plugin};
use remacs_diagnostics::DiagnosticsPlugin;
use remacs_dired::{DiredPlugin, list_directory};
use remacs_documentation::DocumentationPlugin;
use remacs_editing::EditingPlugin;
use remacs_files::FilesPlugin;
use remacs_format::FormatPlugin;
use remacs_git::GitPlugin;
use remacs_languages::{GRAMMARS, LanguagesPlugin, PRETTIER_MODES, SERVERS};
use remacs_lsp::LspPlugin;
use remacs_minibuffer::MinibufferPlugin;
use remacs_prettier::PrettierPlugin;
use remacs_search::SearchPlugin;
use remacs_syntax::SyntaxPlugin;
use remacs_xref::XrefPlugin;

use assets::Assets;
use editor_view::EditorView;
use settings::{ReadSettings, read_settings, report, watch_settings};

const BUILTIN_PLUGINS: &[&dyn Plugin] = &[
    &EditingPlugin,
    &MinibufferPlugin,
    &FilesPlugin,
    &SearchPlugin,
    &DiredPlugin,
    &GitPlugin,
    &LanguagesPlugin,
    &SyntaxPlugin::new(GRAMMARS),
    &DiagnosticsPlugin,
    // Before their backends, so that what they ask after a command is sent in the same pass.
    &CompletionPlugin,
    &DocumentationPlugin,
    &XrefPlugin,
    &ActionsPlugin,
    &FormatPlugin,
    // Format backends, asked in order; language servers format what the others leave.
    &PrettierPlugin::new(PRETTIER_MODES),
    &LspPlugin::new(SERVERS),
];

fn main() -> ExitCode {
    let ReadSettings {
        settings,
        ui,
        errors,
    } = read_settings();

    let mut editor = Editor::new(BUILTIN_PLUGINS);
    editor.set_settings(settings);

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
    report(&errors, &mut editor);

    let cursor_height = ui.cursor.height;
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
                window_background: WindowBackgroundAppearance::Blurred,
                ..Default::default()
            };
            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| EditorView::new(editor, cursor_height, window, cx));
                window.focus(view.read(cx).focus_handle());
                watch_settings(view.downgrade(), cx);
                view
            })
            .expect("failed to open window");
            cx.activate(true);
        });
    ExitCode::SUCCESS
}
