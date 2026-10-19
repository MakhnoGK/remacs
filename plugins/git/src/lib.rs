//! Built-in plugin: Git working trees, read with the `git` command on background threads
//! and kept current from file system events, like Emacs magit.

mod branch;
mod buffer_changes;
mod command;
mod commands;
mod commit;
mod commit_view;
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
mod reads;
mod repositories;
mod resolve;
mod settings;
mod stash;
mod status;
mod status_view;
mod sync;
mod untracked;

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_core::{
    Buffer, ChangedLines, Command, Editor, FileStatus, Keymap, MenuItem, MinorKeymap, Plugin,
    Registry, VersionControl,
};

use crate::repositories::Repositories;
use crate::status_view::{GIT_MODE, TOGGLE, VISIT};

pub struct GitPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new("git-status", "Show the status of the Git working tree", commands::status),
    Command::new(TOGGLE, "Fold or unfold the section at point, or show the file's diff", commands::toggle),
    Command::new("git-toggle-all", "Hide every file's diff when one is shown, else show them all", commands::toggle_all),
    Command::new("git-expand-all", "Show the diff of every changed file", commands::expand_all),
    Command::new("git-collapse-all", "Hide the diff of every file", commands::collapse_all),
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
    Command::new("git-history-next-commit", "Select the next older commit of the history", history_view::next_commit),
    Command::new("git-history-previous-commit", "Select the next newer commit of the history", history_view::previous_commit),
    Command::new("git-history-open-commit", "Show what the selected commit of the history changed", history_view::open_commit),
    Command::new("git-history-all-files", "Show the commits of every file in the history again", history_view::all_files),
    Command::new("git-log-file", "Show the commits that changed the visited file", history_view::file_log),
    Command::new(commit_view::TOGGLE, "Fold or unfold the commit's message", commit_view::toggle),
    Command::new(commit_view::VISIT, "Visit the file at point in the working tree, at the diff line", commit_view::visit),
    Command::new("git-commit-next-hunk", "Go to the next hunk of the commit's diff", commit_view::next_hunk),
    Command::new("git-commit-previous-hunk", "Go to the previous hunk of the commit's diff", commit_view::previous_hunk),
    Command::new("git-commit-next-file", "Go to the next file of the commit's diff", commit_view::next_file),
    Command::new("git-commit-previous-file", "Go to the previous file of the commit's diff", commit_view::previous_file),
    Command::new("git-commit-open-file", "Show the file at point as the commit left it", commit_view::open_file),
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

const BINDINGS: &[(&str, &str)] = &[("C-x g", "git-status"), ("C-x v l", "git-log-file")];

const COMMIT_BINDINGS: &[(&str, &str)] = &[
    ("TAB", commit_view::TOGGLE),
    ("RET", commit_view::VISIT),
    ("o", "git-commit-open-file"),
    ("q", "git-quit"),
    ("n", "git-commit-next-hunk"),
    ("p", "git-commit-previous-hunk"),
    ("N", "git-commit-next-file"),
    ("P", "git-commit-previous-file"),
    ("M-n", "git-commit-next-file"),
    ("M-p", "git-commit-previous-file"),
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("Git", "status", "Status", "git-status"),
    MenuItem::new("Git", "status", "Expand All Changes", "git-expand-all").enabled_when(status_shown),
    MenuItem::new("Git", "status", "Collapse All Changes", "git-collapse-all").enabled_when(status_shown),
    MenuItem::new("Git", "history", "File History", "git-log-file").enabled_when(history_view::visits_file),
    MenuItem::new("Git", "history", "All Files' History", "git-history-all-files").enabled_when(status_shown),
    MenuItem::new("Git", "history", "Show Selected Commit", "git-history-open-commit").enabled_when(status_shown),
    MenuItem::new("Git", "history", "Next Hunk", "git-commit-next-hunk").enabled_when(commit_shown),
    MenuItem::new("Git", "history", "Previous Hunk", "git-commit-previous-hunk").enabled_when(commit_shown),
    MenuItem::new("Git", "history", "Next File", "git-commit-next-file").enabled_when(commit_shown),
    MenuItem::new("Git", "history", "Previous File", "git-commit-previous-file").enabled_when(commit_shown),
    MenuItem::new("Git", "changes", "Commit…", "git-commit").enabled_when(status_shown),
    MenuItem::new("Git", "changes", "Stash…", "git-stash").enabled_when(status_shown),
    MenuItem::new("Git", "branches", "Switch Branch…", "git-branch").enabled_when(status_shown),
    MenuItem::new("Git", "branches", "Merge…", "git-merge").enabled_when(status_shown),
    MenuItem::new("Git", "branches", "Rebase…", "git-rebase").enabled_when(status_shown),
    MenuItem::new("Git", "sync", "Pull", "git-pull").enabled_when(status_shown),
    MenuItem::new("Git", "sync", "Push…", "git-push").enabled_when(status_shown),
];

/// The commands that act on a working tree act on the status buffer's.
fn status_shown(editor: &Editor) -> bool {
    commands::shown_status(editor).is_ok()
}

fn commit_shown(editor: &Editor) -> bool {
    editor.window_buffer().mode() == commit_view::COMMIT_MODE
}

const STATUS_BINDINGS: &[(&str, &str)] = &[
    ("TAB", TOGGLE),
    ("S-TAB", "git-toggle-all"),
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
    ("M-n", "git-history-next-commit"),
    ("M-p", "git-history-previous-commit"),
    ("M-RET", "git-history-open-commit"),
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
        registry.menu_items.extend_from_slice(MENU_ITEMS);
        let mut conflict_keymap = Keymap::default();
        for &(keys, command) in CONFLICT_BINDINGS {
            conflict_keymap.bind(keys, command);
        }
        registry.minor_keymaps.push(MinorKeymap {
            active: |editor| editor.buffer_has_keys() && resolve::in_conflict(editor),
            keymap: conflict_keymap,
        });
        let mut status_keymap = Keymap::default();
        for &(keys, command) in STATUS_BINDINGS {
            status_keymap.bind(keys, command);
        }
        registry.minor_keymaps.push(MinorKeymap {
            active: |editor| editor.buffer_has_keys() && editor.buffer().mode() == GIT_MODE,
            keymap: status_keymap,
        });
        let mut commit_keymap = Keymap::default();
        for &(keys, command) in COMMIT_BINDINGS {
            commit_keymap.bind(keys, command);
        }
        registry.minor_keymaps.push(MinorKeymap {
            active: |editor| {
                editor.buffer_has_keys() && editor.buffer().mode() == commit_view::COMMIT_MODE
            },
            keymap: commit_keymap,
        });
        registry.version_controls.push(Rc::new(Git));
        registry
            .settings_hooks
            .push(Rc::new(settings::read_settings));
        registry
            .post_command_hooks
            .push(Rc::new(repositories::track));
        registry.post_command_hooks.push(Rc::new(status_view::sync));
        registry.post_command_hooks.push(Rc::new(reads::pick_up));
        registry
            .post_command_hooks
            .push(Rc::new(buffer_changes::sync));
        registry.outlines.push(Rc::new(status_view::outline));
        registry.decorators.push(Rc::new(commit_view::decorate));
        registry.line_extras.push(Rc::new(commit_view::line_extras));
        registry.text_controls.push(Rc::new(commit_view::controls));
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

    fn line_changes(&self, buffer: &Buffer, lines: Range<usize>) -> Vec<ChangedLines> {
        buffer_changes::line_changes(buffer, lines)
    }

    fn record_moves(&self, editor: &mut Editor, moves: &[(PathBuf, PathBuf)]) {
        repositories::record_moves(editor, moves);
    }
}

#[cfg(test)]
mod tests;
