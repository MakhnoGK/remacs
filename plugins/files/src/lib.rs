//! Built-in plugin: visiting and saving files, switching and killing buffers, quitting.

mod buffers;
mod file_index;
mod find_file;
mod project;
mod saving;

use remacs_core::{Command, Plugin, Registry};

pub use find_file::directory_candidates;
pub use project::{abbreviate_home, default_directory, expand_home, with_slash};

pub struct FilesPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new("find-file", "Visit a file of the project, or any file by path", find_file::find_file),
    Command::new("switch-to-buffer", "Show another buffer in the window", buffers::switch_to_buffer),
    Command::new("kill-buffer", "Kill a buffer, asking first if it is modified", buffers::kill_buffer),
    Command::new("save-buffer", "Save current buffer in its visited file", saving::save_buffer),
    Command::new("write-file", "Write current buffer into a new file and visit it", saving::write_file),
    Command::new("save-some-buffers", "Offer to save each modified buffer", saving::save_some_buffers),
    Command::new("save-buffers-kill-terminal", "Offer to save modified buffers, then exit", saving::save_buffers_kill_terminal),
];

const BINDINGS: &[(&str, &str)] = &[
    ("C-x C-f", "find-file"),
    ("s-o", "find-file"),
    ("C-x b", "switch-to-buffer"),
    ("C-x k", "kill-buffer"),
    ("C-x C-s", "save-buffer"),
    ("s-s", "save-buffer"),
    ("C-x C-w", "write-file"),
    ("s-S", "write-file"),
    ("C-x s", "save-some-buffers"),
    ("C-x C-c", "save-buffers-kill-terminal"),
    ("s-q", "save-buffers-kill-terminal"),
];

impl Plugin for FilesPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
    }
}

#[cfg(test)]
mod tests;
