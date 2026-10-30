mod sections;
mod ui;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{App, FontWeight, SharedString, WeakEntity, set_family_weights, set_text_strokes};
use remacs_config::{Section, Settings, SettingsError, config_dir, home, themes_dir, watch};
use remacs_core::Editor;
use remacs_theme::font;
use remacs_theme::{Appearance, DEFAULT, Theme, cursor, glass};

use crate::editor_view::EditorView;
use crate::frame;
pub(crate) use sections::settings_sections;
pub use sections::*;
pub use ui::UiSettings;

/// Changes this close together are read once, e.g. an editor's save done in steps.
const SETTLE_TIME: Duration = Duration::from_millis(100);

/// What the settings files hold, with the `ui` section read, and what is wrong with them.
pub struct ReadSettings {
    /// For the plugins.
    pub settings: Settings,
    pub ui: UiSettings,
    pub errors: Vec<SettingsError>,
}

/// The section `T` of `settings`, else its defaults, with the error kept in `errors`.
pub fn section<T: Section>(settings: &Settings, errors: &mut Vec<SettingsError>) -> T {
    settings.section().unwrap_or_else(|(error, defaults)| {
        errors.push(error);
        defaults
    })
}

/// The desktop's own monospace or sans-serif family in place of the macOS one the defaults
/// name, which other systems lack.
#[cfg(not(target_os = "macos"))]
fn desktop_font(family: &str) -> Option<String> {
    let generic = if *family == *font::CodeFont::default().family {
        "monospace"
    } else if family == font::DEFAULT_UI {
        "sans-serif"
    } else {
        return None;
    };
    let output = std::process::Command::new("fc-match")
        .args(["--format=%{family[0]}", generic])
        .output()
        .ok()?;
    String::from_utf8(output.stdout).ok().filter(|family| !family.is_empty())
}

/// Reads the settings and applies the process-wide part of `ui`, the theme as for macOS in
/// `system` appearance; the view and the plugins apply the rest.
pub fn read_settings(system: Appearance) -> ReadSettings {
    let home = home();
    let (settings, mut errors) =
        home.as_deref().map(|home| Settings::load(&config_dir(home))).unwrap_or_default();
    let mut ui: UiSettings = section(&settings, &mut errors);
    let theme = read_theme(ui.theme.shown(system), home.as_deref(), &mut errors);
    remacs_theme::install(&theme);
    // Only the Metal renderer blurs (ADR 0035); unblurred, the desktop would show through.
    if cfg!(not(target_os = "macos")) {
        ui.glass = GlassSettings { opacity: Some(1.0), blur: 0 };
    }
    glass::set_window_opacity(*ui.glass.opacity.get_or_insert(theme.opacity));
    #[cfg(not(target_os = "macos"))]
    for family in [&mut ui.font.family, &mut ui.ui_font.family] {
        if let Some(desktop) = desktop_font(family) {
            *family = desktop;
        }
    }
    let code_font = ui.font.code_font();
    set_family_weights(vec![
        (SharedString::from(Arc::clone(&code_font.family)), FontWeight(code_font.weight)),
        (SharedString::from(ui.ui_font.family.clone()), FontWeight(ui.ui_font.weight.value())),
    ]);
    font::set_code(code_font);
    font::set_ui(&ui.ui_font.family, ui.ui_font.size);
    set_text_strokes(ui.font.strokes.into());
    glass::set_blurs(ui.glass.blur > 0);
    cursor::set_radius(ui.cursor.radius);
    ReadSettings { settings, ui, errors }
}

/// The theme `ui.toml` names, else the default one, the error reported.
fn read_theme(name: &str, home: Option<&Path>, errors: &mut Vec<SettingsError>) -> Theme {
    let dir = home.map(themes_dir);
    Theme::load(name, dir.as_deref()).unwrap_or_else(|error| {
        let ui_file = || {
            let config = home.map(config_dir).unwrap_or_default();
            config.join(format!("{}.toml", UiSettings::NAME))
        };
        errors.push(SettingsError {
            file: error.file.unwrap_or_else(ui_file),
            line: error.line,
            message: error.message,
        });
        Theme::built_in(DEFAULT)
    })
}

/// Shows the first error; all go to stderr.
pub fn report(errors: &[SettingsError], editor: &mut Editor) {
    for error in errors {
        eprintln!("remacs: {error}");
    }
    if let Some(error) = errors.first() {
        editor.set_error(error.to_string());
    }
}

/// Applies the settings again whenever their files or the themes change, for as long as
/// `view` lives.
pub fn watch_settings(view: WeakEntity<EditorView>, cx: &mut App) {
    let Some(home) = home() else {
        return;
    };
    let (changed, mut changes) = mpsc::unbounded();
    let Ok(watchers) = [config_dir(&home), themes_dir(&home)]
        .iter()
        .map(|dir| {
            let changed = changed.clone();
            watch(dir, move || {
                let _ = changed.unbounded_send(());
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .inspect_err(|error| eprintln!("remacs: settings are not watched: {error}"))
    else {
        return;
    };
    cx.spawn(async move |cx| {
        let _watchers = watchers;
        while changes.next().await.is_some() {
            cx.background_executor().timer(SETTLE_TIME).await;
            while changes.try_recv().is_ok() {}
            let read = read_settings(frame::system_appearance());
            if view.update(cx, |view, cx| view.apply_settings(read, cx)).is_err() {
                break;
            }
        }
    })
    .detach();
}
