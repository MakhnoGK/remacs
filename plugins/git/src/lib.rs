//! Built-in plugin: Git working trees, read with the `git` command on background threads
//! and kept current from file system events, like Emacs magit.

mod command;
mod ignore_rules;
mod monitor;
mod moves;
mod repositories;
mod status;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_core::{Editor, FileStatus, Plugin, Registry, VersionControl};

use crate::repositories::Repositories;

pub struct GitPlugin;

impl Plugin for GitPlugin {
    fn register(&self, registry: &mut Registry) {
        registry.version_controls.push(Rc::new(Git));
        registry
            .post_command_hooks
            .push(Rc::new(repositories::track));
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
