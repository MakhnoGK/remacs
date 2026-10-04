use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use remacs_core::Editor;

use crate::monitor::{Monitor, Update, Wants};
use crate::status::Snapshot;
use crate::status_view;

/// The working trees the buffers are in, each watched while a buffer is in it.
#[derive(Default)]
pub struct Repositories {
    /// Directories of buffers, with the working tree each is in, if any.
    roots: HashMap<PathBuf, Option<PathBuf>>,
    open: HashMap<PathBuf, Repository>,
}

pub struct Repository {
    pub monitor: Monitor,
    /// `None` until first read.
    pub snapshot: Option<Snapshot>,
    /// Counts the snapshots read, so views know when to redraw.
    pub generation: u64,
    wants: Wants,
}

impl Repositories {
    pub fn get(&self, root: &Path) -> Option<&Repository> {
        self.open.get(root)
    }

    /// The open working tree `path` is in, innermost first, with `path` relative to it.
    pub fn find<'a>(&'a self, path: &'a Path) -> Option<(&'a Path, &'a Repository, &'a Path)> {
        self.open
            .iter()
            .filter_map(|(root, repository)| {
                Some((root.as_path(), repository, path.strip_prefix(root).ok()?))
            })
            .max_by_key(|(root, ..)| root.as_os_str().len())
    }
}

/// Watches the working trees of the buffers' directories and stops watching those no
/// buffer is in any more; takes in what the monitors found. Runs after every command.
pub fn track(editor: &mut Editor) {
    let directories: HashSet<PathBuf> = editor
        .buffers()
        .iter()
        .filter_map(|buffer| buffer.default_directory().map(Path::to_path_buf))
        .collect();
    let mut wanted = status_view::wants(editor);
    let notifier = editor.notifier();
    let repositories = editor.plugin_state::<Repositories>();
    repositories
        .roots
        .retain(|directory, _| directories.contains(directory));
    for directory in directories {
        repositories
            .roots
            .entry(directory)
            .or_insert_with_key(|directory| working_tree(directory));
    }
    let used: HashSet<&PathBuf> = repositories.roots.values().flatten().collect();
    repositories.open.retain(|root, _| used.contains(root));
    for root in used {
        if !repositories.open.contains_key(root) {
            let wants = wanted.get(root).cloned().unwrap_or_default();
            let repository = Repository {
                monitor: Monitor::start(root.clone(), wants.clone(), notifier.clone()),
                snapshot: None,
                generation: 0,
                wants,
            };
            repositories.open.insert(root.clone(), repository);
        }
    }
    let mut failure = None;
    for (root, repository) in &mut repositories.open {
        let wants = wanted.remove(root).unwrap_or_default();
        if wants != repository.wants {
            repository.monitor.want(wants.clone());
            repository.wants = wants;
        }
        for update in repository.monitor.updates() {
            match update {
                Update::Status(mut snapshot) => {
                    carry_resolved(repository.snapshot.as_ref(), &mut snapshot);
                    repository.snapshot = Some(*snapshot);
                    repository.generation += 1;
                }
                Update::Failed(error) => failure = Some(error),
            }
        }
    }
    if let Some(failure) = failure {
        editor.set_message(failure);
    }
}

/// While a rebase step or a merge goes on, files that had conflicts in it stay known as
/// such once they are resolved.
fn carry_resolved(previous: Option<&Snapshot>, snapshot: &mut Snapshot) {
    if snapshot.operation.is_none() {
        return;
    }
    if let Some(previous) = previous.filter(|previous| previous.operation == snapshot.operation) {
        snapshot.resolved.extend(previous.resolved.iter().cloned());
    }
    let conflicted = snapshot
        .files
        .iter()
        .filter(|(_, entry)| entry.conflicted)
        .map(|(path, _)| path.clone());
    snapshot.resolved.extend(conflicted.collect::<Vec<_>>());
}

/// Tells the monitor of the working tree the moves are in; moves out of it are left out.
pub fn record_moves(editor: &mut Editor, moves: &[(PathBuf, PathBuf)]) {
    let Some(repositories) = editor.find_plugin_state::<Repositories>() else {
        return;
    };
    let mut by_root: HashMap<&Path, Vec<(PathBuf, PathBuf)>> = HashMap::new();
    for (from, to) in moves {
        let Some((root, _, relative_from)) = repositories.find(from) else {
            continue;
        };
        if let Ok(relative_to) = to.strip_prefix(root) {
            by_root
                .entry(root)
                .or_default()
                .push((relative_from.to_path_buf(), relative_to.to_path_buf()));
        }
    }
    for (root, moves) in by_root {
        repositories.open[root].monitor.record_moves(moves);
    }
}

/// The nearest enclosing directory with a `.git`, a directory or, for a linked working
/// tree or a submodule, a file.
pub fn working_tree(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}
