//! Built-in plugin: the programs remacs runs but does not ship, like debug adapters and
//! language servers, downloaded from their GitHub releases or installed by a package
//! manager (npm, go, pip…, see `installers.toml`) when they are first wanted. A download
//! is checked against the release's checksum; all are kept under
//! `~/.config/remacs/cache/tools/`, one directory per version; `tools.toml` can pin a
//! version, name a program of one's own, or add an installer.

mod catalog;
mod install;
mod installers;
mod panel;
mod settings;
mod state;

use std::env;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_config::{cache_dir, config_dir};
use remacs_core::{Editor, MenuItem, Plugin, Registry, StatusItem, StatusSegment, Tone};

use crate::settings::ToolsSettings;
use crate::state::{Progress, Tools, tools};

pub use catalog::{CODELLDB, InstallSpec, JS_DEBUG, Source, Tool, ToolKind};
pub use state::Program;

pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in panel::COMMANDS {
            registry.commands.add(*command);
        }
        registry.menu_items.push(MenuItem::new("View", "panels", "Tools", panel::OPEN));
        registry.settings_hooks.push(Rc::new(read_settings));
        registry.post_command_hooks.push(Rc::new(pick_up));
        registry.status_providers.push(Rc::new(status));
    }
}

/// Installs tools under `home`'s cache directory, its settings file being `tools.toml`.
pub fn keep_tools_in(editor: &mut Editor, home: &Path) {
    state::keep_in(editor, cache_dir(home).join("tools"), config_dir(home).join("tools.toml"));
}

/// Calls `then` with the program of the tool called `name` once it is there, downloading
/// it first if it is not; or with why it could not be got.
pub fn ensure(
    editor: &mut Editor,
    name: &str,
    then: impl FnOnce(&mut Editor, Result<PathBuf, String>) + 'static,
) {
    state::ensure(editor, name, Box::new(then));
}

/// The program of the tool called `name` if it is there, without downloading it.
pub fn installed(editor: &Editor, name: &str) -> Option<Program> {
    editor.find_plugin_state::<Tools>()?.installed_program(name)
}

/// Whether `tool` can be installed here, e.g. npm is there for an npm package.
pub fn can_install(editor: &Editor, tool: &Tool) -> bool {
    editor.find_plugin_state::<Tools>().is_some_and(|tools| tools.can_install(tool))
}

/// Makes `tools` the tools of `kind`, e.g. the language servers of the servers' catalog;
/// `tools.toml` naming a tool that is none of them is an error.
pub fn provide(editor: &mut Editor, kind: ToolKind, tools: &[&'static Tool]) {
    let state = self::tools(editor);
    state.provide(kind, tools);
    // Grammars are provided before the servers, whose names would not be known yet.
    if kind == ToolKind::Grammar {
        return;
    }
    let unknown = state.settings.tools.keys().find(|name| state.entry(name).is_none()).cloned();
    if let Some(name) = unknown {
        editor.set_error(format!("tools.toml: no tool is called {name}"));
    }
}

/// `program` as a path: itself if it has a slash, else where `PATH` finds it.
pub fn find_on_path(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        return Some(PathBuf::from(program));
    }
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(program))
        .find(|path| is_executable(path))
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn read_settings(editor: &mut Editor) {
    let settings = editor.settings_section::<ToolsSettings>();
    let tools = tools(editor);
    if tools.settings != settings {
        tools.set_settings(settings);
    }
}

fn pick_up(editor: &mut Editor) {
    if state::pick_up(editor) && panel::is_open(editor) {
        editor.refresh_minibuffer();
    }
}

/// How the download going on goes; a click shows the Tools panel.
fn status(editor: &Editor) -> Option<StatusSegment> {
    let tools = editor.find_plugin_state::<Tools>()?;
    let item = tools.entries.iter().find_map(|entry| {
        let name = entry.tool.name;
        let (text, percent) = match &entry.progress {
            Progress::Downloading { version, percent: Some(percent) } => {
                let version = version.as_deref().unwrap_or_default();
                (format!("Downloading {name} {version} — {percent}%"), Some(*percent))
            }
            Progress::Downloading { percent: None, .. } => (format!("Looking up {name}…"), None),
            Progress::Installing { version } => (format!("Installing {name} {version}…"), None),
            Progress::Idle | Progress::Failed(_) => return None,
        };
        let started = entry.job.as_ref()?.started;
        Some(StatusItem { tone: Tone::Accent, ..StatusItem::working(text, started, percent) })
    })?;
    Some(StatusSegment { items: vec![item], command: Some(panel::OPEN), active: true, code: true })
}

#[cfg(test)]
mod tests;
