//! Built-in plugin: directory listings drawn as tables, read in the background and kept
//! current from file system events, like Emacs dired.

mod commands;
mod entry;
mod listing;
mod reader;

use std::path::Path;
use std::rc::Rc;

use remacs_core::{Command, CommandResult, Editor, Keymap, MinorKeymap, Plugin, Registry};

use crate::listing::DIRED_MODE;

pub struct DiredPlugin;

/// Shows a listing of `directory`, e.g. one given on the command line.
pub fn list_directory(editor: &mut Editor, directory: &Path) -> CommandResult {
    commands::open_directory(editor, directory, None, false)
}

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new("dired", "List a directory", commands::dired),
    Command::new("dired-jump", "List the directory of the current file, with point on it", commands::dired_jump),
    Command::new(commands::FIND_FILE, "Visit the file or list the directory at point", commands::find_file),
    Command::new("dired-up-directory", "List the parent directory", commands::up_directory),
    Command::new("dired-revert", "Read the directory again", commands::revert),
    Command::new("dired-sort-toggle", "Sort by the next of name, modified time and size", commands::sort_toggle),
    Command::new("dired-sort-by-name", "Sort the listing by name", commands::sort_by_name),
    Command::new("dired-sort-by-modified", "Sort the listing by modified time, newest first", commands::sort_by_modified),
    Command::new("dired-sort-by-size", "Sort the listing by size, biggest first", commands::sort_by_size),
];

const BINDINGS: &[(&str, &str)] = &[("C-x d", "dired"), ("C-x C-j", "dired-jump")];

const DIRED_BINDINGS: &[(&str, &str)] = &[
    ("RET", commands::FIND_FILE),
    ("f", commands::FIND_FILE),
    ("e", commands::FIND_FILE),
    ("^", "dired-up-directory"),
    ("g", "dired-revert"),
    ("s", "dired-sort-toggle"),
    ("n", "next-line"),
    ("SPC", "next-line"),
    ("p", "previous-line"),
];

impl Plugin for DiredPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        let mut keymap = Keymap::default();
        for &(keys, command) in DIRED_BINDINGS {
            keymap.bind(keys, command);
        }
        registry.minor_keymaps.push(MinorKeymap {
            active: |editor| editor.buffer().mode() == DIRED_MODE,
            keymap,
        });
        registry.tables.push(Rc::new(listing::table));
        registry.status_providers.push(Rc::new(listing::status));
        registry
            .post_command_hooks
            .push(Rc::new(listing::pick_up_listings));
    }
}

#[cfg(test)]
mod tests;
