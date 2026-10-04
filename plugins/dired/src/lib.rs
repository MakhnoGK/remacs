//! Built-in plugin: directory listings drawn as tables, read in the background and kept
//! current from file system events, like Emacs dired.

mod commands;
mod delete;
mod entry;
mod file_ops;
mod jobs;
mod listing;
mod marks;
mod preview;
mod preview_reader;
mod reader;
mod transfer;

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
    Command::new("dired-mark", "Mark the file at point and move to the next", marks::mark),
    Command::new("dired-unmark", "Unmark the file at point and move to the next", marks::unmark),
    Command::new("dired-unmark-backward", "Move to the previous file and unmark it", marks::unmark_backward),
    Command::new("dired-unmark-all-marks", "Remove every mark and flag", marks::unmark_all),
    Command::new("dired-toggle-marks", "Mark the unmarked files and unmark the marked ones", marks::toggle_marks),
    Command::new("dired-flag-file-deletion", "Flag the file at point for deletion and move to the next", marks::flag),
    Command::new("dired-flag-backup-files", "Flag backup files (name~) for deletion", marks::flag_backup_files),
    Command::new("dired-flag-auto-save-files", "Flag auto-save files (#name#) for deletion", marks::flag_auto_save_files),
    Command::new("dired-do-flagged-delete", "Delete the flagged files, after asking", delete::delete_flagged),
    Command::new("dired-do-delete", "Delete the marked files, or the one at point, after asking", delete::delete_marked),
    Command::new("dired-create-directory", "Create a folder in the listed directory", commands::create_directory),
    Command::new("dired-do-copy", "Copy the marked files, or the one at point, elsewhere", transfer::copy),
    Command::new("dired-do-rename", "Move or rename the marked files, or the one at point", transfer::rename),
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
    ("m", "dired-mark"),
    ("u", "dired-unmark"),
    ("DEL", "dired-unmark-backward"),
    ("U", "dired-unmark-all-marks"),
    ("t", "dired-toggle-marks"),
    ("d", "dired-flag-file-deletion"),
    ("~", "dired-flag-backup-files"),
    ("#", "dired-flag-auto-save-files"),
    ("x", "dired-do-flagged-delete"),
    ("D", "dired-do-delete"),
    ("+", "dired-create-directory"),
    ("C", "dired-do-copy"),
    ("R", "dired-do-rename"),
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
        registry
            .status_providers
            .push(Rc::new(listing::marks_status));
        registry.status_providers.push(Rc::new(listing::status));
        registry
            .post_command_hooks
            .push(Rc::new(listing::pick_up_listings));
        registry.post_command_hooks.push(Rc::new(preview::sync));
        registry.post_command_hooks.push(Rc::new(jobs::pick_up));
    }
}

#[cfg(test)]
mod tests;
