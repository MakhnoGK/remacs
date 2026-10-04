use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use remacs_core::Editor;

use crate::monitor::{Monitor, Update};
use crate::status::Snapshot;

/// The working trees the buffers are in, each watched while a buffer is in it.
#[derive(Default)]
pub struct Repositories {
    /// Directories of buffers, with the working tree each is in, if any.
    roots: HashMap<PathBuf, Option<PathBuf>>,
    open: HashMap<PathBuf, Repository>,
}

pub struct Repository {
    monitor: Monitor,
    /// `None` until first read.
    pub snapshot: Option<Snapshot>,
}

impl Repositories {
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
            let repository = Repository {
                monitor: Monitor::start(root.clone(), notifier.clone()),
                snapshot: None,
            };
            repositories.open.insert(root.clone(), repository);
        }
    }
    let mut failure = None;
    for repository in repositories.open.values_mut() {
        for update in repository.monitor.updates() {
            match update {
                Update::Status(snapshot) => repository.snapshot = Some(snapshot),
                Update::Failed(error) => failure = Some(error),
            }
        }
    }
    if let Some(failure) = failure {
        editor.set_message(failure);
    }
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
fn working_tree(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}
