//! Built-in plugin: the programs remacs runs but does not ship, like debug adapters,
//! downloaded from their GitHub releases the first time they are needed. Each is checked
//! against the release's checksum and kept under `~/.config/remacs/cache/tools/`, one
//! directory per version; `tools.toml` can pin a version or name a program of one's own.

mod catalog;
mod install;
mod panel;
mod settings;
mod state;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_config::{cache_dir, config_dir};
use remacs_core::{Editor, MenuItem, Plugin, Registry, StatusItem, StatusSegment, Tone};

use crate::settings::ToolsSettings;
use crate::state::{Progress, Tools, tools};

pub use catalog::{CODELLDB, JS_DEBUG, Tool, ToolKind};

pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in panel::COMMANDS {
            registry.commands.add(*command);
        }
        registry.keymap.bind("C-c t", panel::OPEN);
        registry
            .menu_items
            .push(MenuItem::new("View", "panels", "Tools", panel::OPEN));
        registry.settings_hooks.push(Rc::new(read_settings));
        registry.post_command_hooks.push(Rc::new(pick_up));
        registry.status_providers.push(Rc::new(status));
    }
}

/// Installs tools under `home`'s cache directory, its settings file being `tools.toml`.
pub fn keep_tools_in(editor: &mut Editor, home: &Path) {
    state::keep_in(
        editor,
        cache_dir(home).join("tools"),
        config_dir(home).join("tools.toml"),
    );
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

fn read_settings(editor: &mut Editor) {
    let settings = editor.settings_section::<ToolsSettings>();
    let tools = tools(editor);
    if tools.settings != settings {
        tools.settings = settings;
        tools.scan();
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
    let text = tools.entries.iter().find_map(|entry| {
        let name = entry.tool.name;
        match &entry.progress {
            Progress::Downloading {
                version,
                percent: Some(percent),
            } => {
                let version = version.as_deref().unwrap_or_default();
                Some(format!("Downloading {name} {version} — {percent}%"))
            }
            Progress::Downloading { percent: None, .. } => Some(format!("Looking up {name}…")),
            Progress::Unpacking { version } => Some(format!("Unpacking {name} {version}…")),
            Progress::Idle | Progress::Failed(_) => None,
        }
    })?;
    Some(StatusSegment {
        items: vec![StatusItem::toned(text, Tone::Accent)],
        command: Some(panel::OPEN),
        active: true,
        code: true,
    })
}

#[cfg(test)]
mod tests;
