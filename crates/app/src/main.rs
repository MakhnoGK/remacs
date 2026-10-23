mod assets;
mod bottom_row;
mod changes_view;
mod code_wrap;
mod composition;
mod conflict_view;
mod dialog_view;
mod display_line;
mod editor_view;
mod environment;
mod frame;
mod highlights;
mod key_input;
#[cfg(test)]
mod keymap_rules;
mod logs;
mod menu_bar;
mod minibuffer_view;
mod opening;
mod outline_view;
mod pane_view;
mod popup_view;
mod preview_view;
mod screen_view;
mod settings;
mod smooth_scroll;
mod table_view;
mod tabs_view;
mod text_zoom;
mod transition;
mod version;
mod widgets;

use std::path::PathBuf;
use std::process::ExitCode;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, prelude::*, px,
    size,
};
use remacs_actions::ActionsPlugin;
use remacs_completion::CompletionPlugin;
use remacs_config::{cache_dir, home, state_dir};
use remacs_core::{Editor, Plugin};
use remacs_debug::{DebugPlugin, keep_breakpoints_in};
use remacs_diagnostics::DiagnosticsPlugin;
use remacs_dired::DiredPlugin;
use remacs_documentation::DocumentationPlugin;
use remacs_echo::EchoPlugin;
use remacs_editing::EditingPlugin;
use remacs_files::{FilesPlugin, keep_session_in};
use remacs_format::FormatPlugin;
use remacs_frame::FramePlugin;
use remacs_git::GitPlugin;
use remacs_languages::{GRAMMARS, INJECTIONS, LanguagesPlugin, PRETTIER_MODES, SERVERS};
use remacs_lsp::{LspPlugin, keep_offers_in};
use remacs_minibuffer::MinibufferPlugin;
use remacs_navigation::NavigationPlugin;
use remacs_prettier::PrettierPlugin;
use remacs_rename::RenamePlugin;
use remacs_run::{RunPlugin, keep_configurations_in};
use remacs_search::SearchPlugin;
use remacs_syntax::SyntaxPlugin;
use remacs_terminal::TerminalPlugin;
use remacs_tools::{ToolsPlugin, keep_tools_in};
use remacs_xref::XrefPlugin;

use assets::Assets;
use editor_view::EditorView;
use environment::EnvironmentSettings;
use logs::LogSettings;
use opening::{file_paths, open};
use settings::{ReadSettings, read_settings, report, watch_settings};
use version::{COMMIT, VERSION};

const BUILTIN_PLUGINS: &[&dyn Plugin] = &[
    // First, as it lays out the menu bar the others add items to.
    &FramePlugin,
    &EditingPlugin,
    &NavigationPlugin,
    &MinibufferPlugin,
    &FilesPlugin,
    &SearchPlugin,
    &DiredPlugin,
    &GitPlugin,
    &LanguagesPlugin,
    &SyntaxPlugin::new(GRAMMARS, INJECTIONS),
    &DiagnosticsPlugin,
    // Before the run plugin, which then shows in the same pass what the debugger did.
    &DebugPlugin,
    &RunPlugin,
    &TerminalPlugin,
    &ToolsPlugin,
    // Before their backends, so that what they ask after a command is sent in the same pass.
    &CompletionPlugin,
    &DocumentationPlugin,
    &XrefPlugin,
    &ActionsPlugin,
    &RenamePlugin,
    &FormatPlugin,
    // Format backends, asked in order; language servers format what the others leave.
    &PrettierPlugin::new(PRETTIER_MODES),
    &LspPlugin::new(SERVERS),
    // Last, so that *Messages* shows what the others said in the same pass.
    &EchoPlugin,
];

fn main() -> ExitCode {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--version")
    {
        println!("remacs {VERSION} ({COMMIT})");
        return ExitCode::SUCCESS;
    }
    let ReadSettings {
        settings,
        ui,
        mut errors,
    } = read_settings(frame::system_appearance());
    let logs = settings
        .section::<LogSettings>()
        .unwrap_or_else(|(error, defaults)| {
            errors.push(error);
            defaults
        });
    // First, so that what goes wrong from here on is kept.
    let crashed = home().and_then(|home| logs::start(&state_dir(&home), &logs));
    // Before the plugins start their threads; shown once the window is open.
    let (reported, mut reports) = mpsc::unbounded();
    logs::report_to(reported);
    let environment =
        settings
            .section::<EnvironmentSettings>()
            .unwrap_or_else(|(error, defaults)| {
                errors.push(error);
                defaults
            });
    // Before the plugins, which start programs that look for others on `PATH`.
    let imported = environment::import(&environment);

    let path = std::env::args_os().nth(1).map(PathBuf::from);
    let mut editor = Editor::new(BUILTIN_PLUGINS);
    editor.set_settings(settings);
    if let Some(home) = home() {
        editor.keep_history_in(state_dir(&home).join("history.json"));
        editor.keep_caches_in(cache_dir(&home));
        keep_configurations_in(&mut editor, &home);
        keep_tools_in(&mut editor, &home);
        keep_offers_in(&mut editor, &home);
        keep_breakpoints_in(&mut editor, &home);
        let session = state_dir(&home).join("session.json");
        keep_session_in(&mut editor, session, path.as_deref());
    }

    if let Some(path) = path
        && let Err(error) = open(&mut editor, &path)
    {
        eprintln!("remacs: {}: {error}", path.display());
        return ExitCode::FAILURE;
    }
    report(&errors, &mut editor);
    if let Err(error) = imported {
        eprintln!("remacs: {error}");
        editor.set_error(format!("Not the login shell's environment: {error}"));
    }
    if let Some(crashed) = crashed {
        editor.set_error(crashed);
    }

    // Finder's and the Dock's, which may come before the window is open.
    let (opened, mut to_open) = mpsc::unbounded();
    let app = Application::new().with_assets(Assets);
    app.on_open_urls(move |urls| {
        let _ = opened.unbounded_send(file_paths(&urls));
    });
    app.run(move |cx: &mut App| {
        cx.on_window_closed(|cx| cx.quit()).detach();
        let bounds = Bounds::centered(None, size(px(1440.0), px(900.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                // Not drawn; macOS shows it in Mission Control and the Window menu.
                title: Some("Remacs".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(18.0), px(16.0))),
            }),
            // Blurred by the radius in the settings, not by the system's fixed material.
            window_background: ui.glass.window_background(),
            ..Default::default()
        };
        let window = cx
            .open_window(options, |window, cx| {
                let view = cx.new(|cx| EditorView::new(editor, &ui, window, cx));
                let gate = view.downgrade();
                window.on_window_should_close(cx, move |_, cx| {
                    gate.update(cx, |view, cx| view.may_quit(cx))
                        .unwrap_or(true)
                });
                window.focus(view.read(cx).focus_handle());
                watch_settings(view.downgrade(), cx);
                view
            })
            .expect("failed to open window");
        // `cx.quit()` comes here too, once the gate has agreed.
        cx.on_should_quit(move |cx| {
            window
                .update(cx, |view, _, cx| view.may_quit(cx))
                .unwrap_or(true)
        });
        cx.spawn(async move |cx| {
            while let Some(paths) = to_open.next().await {
                if window
                    .update(cx, |view, _, cx| view.open_paths(paths, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |cx| {
            while let Some(report) = reports.next().await {
                let text = logs::reported(&report);
                if window
                    .update(cx, |view, _, cx| view.show_error(text, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.activate(true);
    });
    ExitCode::SUCCESS
}
