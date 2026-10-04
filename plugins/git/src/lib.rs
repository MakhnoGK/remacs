//! Built-in plugin: Git working trees, read with the `git` command on background threads
//! and kept current from file system events, like Emacs magit.

mod branch;
mod command;
mod commands;
mod commit;
mod details;
mod diff;
mod history;
mod history_view;
mod ignore_rules;
mod integrate;
mod monitor;
mod moves;
mod repositories;
mod status;
mod status_view;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_core::{
    Command, Editor, FileStatus, Keymap, MinorKeymap, Plugin, Registry, VersionControl,
};

use crate::repositories::Repositories;
use crate::status_view::{GIT_MODE, TOGGLE, VISIT};

pub struct GitPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new("git-status", "Show the status of the Git working tree", commands::status),
    Command::new(TOGGLE, "Fold or unfold the section at point, or show the file's diff", commands::toggle),
    Command::new("git-stage", "Stage the section, file or hunk at point", commands::stage),
    Command::new("git-unstage", "Unstage the section, file or hunk at point", commands::unstage),
    Command::new("git-discard", "Throw away the unstaged change at point, after asking", commands::discard),
    Command::new(VISIT, "Visit the file at point, at the diff line", commands::visit),
    Command::new("git-refresh", "Read the status again", commands::refresh),
    Command::new("git-quit", "Show the buffer shown before the status", commands::quit),
    Command::new("git-commit", "Commit what is staged, with a message", commit::commit),
    Command::new("git-branch", "Switch to a branch, or create one from a base", branch::branch),
    Command::new("git-rebase", "Rebase the branch onto its base or another, choosing what to replay", integrate::rebase),
    Command::new("git-merge", "Merge the base or another branch into this one", integrate::merge),
    Command::new(integrate::NEXT_TARGET, "Bring in the next branch instead", integrate::next_target),
    Command::new(integrate::PICK, "Replay the selected commit as it is", integrate::pick),
    Command::new(integrate::SQUASH, "Fold the selected commit into the one before, messages combined", integrate::squash),
    Command::new(integrate::FIXUP, "Fold the selected commit into the one before, its message dropped", integrate::fixup),
    Command::new(integrate::DROP, "Leave the selected commit out", integrate::drop_step),
    Command::new(integrate::EARLIER, "Replay the selected commit one earlier", integrate::earlier),
    Command::new(integrate::LATER, "Replay the selected commit one later", integrate::later),
    Command::new("git-history-scope", "Show the history of all branches, or of the current one", history_view::toggle_scope),
    Command::new("git-history-search", "Show the commits whose messages hold some words", history_view::search),
    Command::new("git-history-next-page", "Scroll the history to older commits", history_view::next_page),
    Command::new("git-history-previous-page", "Scroll the history to newer commits", history_view::previous_page),
];

const BINDINGS: &[(&str, &str)] = &[("C-x g", "git-status")];

const STATUS_BINDINGS: &[(&str, &str)] = &[
    ("TAB", TOGGLE),
    ("s", "git-stage"),
    ("u", "git-unstage"),
    ("k", "git-discard"),
    ("RET", VISIT),
    ("g", "git-refresh"),
    ("q", "git-quit"),
    ("c", "git-commit"),
    ("b", "git-branch"),
    ("m", "git-merge"),
    ("r", "git-rebase"),
    ("l", "git-history-scope"),
    ("/", "git-history-search"),
    ("C-M-v", "git-history-next-page"),
    ("C-M-V", "git-history-previous-page"),
    ("n", "next-line"),
    ("p", "previous-line"),
];

impl Plugin for GitPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        let mut status_keymap = Keymap::default();
        for &(keys, command) in STATUS_BINDINGS {
            status_keymap.bind(keys, command);
        }
        registry.minor_keymaps.push(MinorKeymap {
            active: |editor| editor.buffer().mode() == GIT_MODE,
            keymap: status_keymap,
        });
        registry.version_controls.push(Rc::new(Git));
        registry
            .post_command_hooks
            .push(Rc::new(repositories::track));
        registry.post_command_hooks.push(Rc::new(status_view::sync));
        registry.outlines.push(Rc::new(status_view::outline));
        registry
            .status_providers
            .push(Rc::new(status_view::branch_status));
        registry
            .status_providers
            .push(Rc::new(status_view::sync_status));
    }
}

struct Git;

impl VersionControl for Git {
    fn name(&self) -> &'static str {
        "Git"
    }

    fn move_command(&self) -> &'static str {
        "git mv"
    }

    fn controls(&self, editor: &Editor, path: &Path) -> bool {
        editor
            .find_plugin_state::<Repositories>()
            .is_some_and(|repositories| repositories.find(path).is_some())
    }

    fn status(&self, editor: &Editor, path: &Path) -> Option<FileStatus> {
        let repositories = editor.find_plugin_state::<Repositories>()?;
        let (_, repository, relative) = repositories.find(path)?;
        repository.snapshot.as_ref()?.status(relative)
    }

    fn record_moves(&self, editor: &mut Editor, moves: &[(PathBuf, PathBuf)]) {
        repositories::record_moves(editor, moves);
    }
}

#[cfg(test)]
mod tests;
