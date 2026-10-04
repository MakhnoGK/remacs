//! Built-in plugin: Git working trees, read with the `git` command on background threads
//! and kept current from file system events, like Emacs magit.

mod branch;
mod command;
mod commands;
mod commit;
mod conflict;
mod conflict_view;
mod details;
mod diff;
mod history;
mod history_view;
mod ignore_rules;
mod integrate;
mod monitor;
mod moves;
mod operation;
mod repositories;
mod resolve;
mod stash;
mod status;
mod status_view;
mod sync;

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
    Command::new("git-keep-ours", "Resolve the conflict to what was there: the upstream when rebasing", resolve::keep_ours),
    Command::new("git-keep-theirs", "Resolve the conflict to what comes in: your commit when rebasing", resolve::keep_theirs),
    Command::new("git-keep-both", "Resolve the conflict to both sides, one after the other", resolve::keep_both),
    Command::new("git-edit-conflict", "Visit the file at the conflict to resolve it by hand", resolve::edit_current),
    Command::new("git-next-conflict", "Open the next conflict", resolve::next_conflict),
    Command::new("git-previous-conflict", "Open the previous conflict", resolve::previous_conflict),
    Command::new("git-continue", "Go on with the rebase or merge once its conflicts are resolved", resolve::continue_operation),
    Command::new(resolve::ABORT_COMMAND, "Give up the rebase or merge, after asking", resolve::abort),
    Command::new(resolve::SKIP_COMMAND, "Leave out the commit the rebase is applying", resolve::skip),
    Command::new("git-push", "Push the branch to its upstream or elsewhere", sync::push),
    Command::new("git-pull", "Pull into the branch from its upstream", sync::pull),
    Command::new(sync::PUSH_UPSTREAM, "Push to the upstream", sync::upstream),
    Command::new(sync::PUSH_ELSEWHERE, "Push to a remote branch read next", sync::elsewhere),
    Command::new(sync::PULL_REBASE, "Pull by rebasing onto what comes in", sync::by_rebase),
    Command::new(sync::PULL_MERGE, "Pull by merging what comes in", sync::by_merge),
    Command::new("git-stash", "Set the changes aside with a message, or act on a stash", stash::stash),
    Command::new(stash::APPLY, "Apply the stash, keeping it", stash::apply),
    Command::new(stash::POP, "Apply the stash and drop it", stash::pop),
    Command::new(stash::BRANCH, "Make a branch where the stash was made and apply it there", stash::branch),
    Command::new(stash::SHOW, "Show the stash's files and diffs in the status", stash::show),
    Command::new(stash::DROP, "Throw the stash away, after asking", stash::drop_stash),
];

/// Over the status keys while point is on a file with conflicts, like Emacs smerge.
const CONFLICT_BINDINGS: &[(&str, &str)] = &[
    ("o", "git-keep-ours"),
    ("t", "git-keep-theirs"),
    ("b", "git-keep-both"),
    ("e", "git-edit-conflict"),
    ("n", "git-next-conflict"),
    ("p", "git-previous-conflict"),
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
    ("F", "git-pull"),
    ("P", "git-push"),
    ("z", "git-stash"),
    ("a", stash::APPLY),
    ("A", stash::POP),
    ("l", "git-history-scope"),
    ("/", "git-history-search"),
    ("C-M-v", "git-history-next-page"),
    ("C-M-V", "git-history-previous-page"),
    ("C-c C-c", "git-continue"),
    ("C-c C-k", resolve::ABORT_COMMAND),
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
        let mut conflict_keymap = Keymap::default();
        for &(keys, command) in CONFLICT_BINDINGS {
            conflict_keymap.bind(keys, command);
        }
        registry.minor_keymaps.push(MinorKeymap {
            active: resolve::in_conflict,
            keymap: conflict_keymap,
        });
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
