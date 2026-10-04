use std::path::{Path, PathBuf};

use crate::editor::Editor;

/// How a file differs from the last commit, like Emacs `vc-state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileState {
    Added,
    Modified,
    Renamed,
    Deleted,
    /// Changed on both sides of a merge, not yet resolved.
    Conflicted,
    Untracked,
}

impl FileState {
    /// As `git status --short` shows it.
    pub fn letter(self) -> &'static str {
        match self {
            FileState::Added => "A",
            FileState::Modified => "M",
            FileState::Renamed => "R",
            FileState::Deleted => "D",
            FileState::Conflicted => "U",
            FileState::Untracked => "?",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FileState::Added => "Added",
            FileState::Modified => "Modified",
            FileState::Renamed => "Renamed",
            FileState::Deleted => "Deleted",
            FileState::Conflicted => "Conflicted",
            FileState::Untracked => "Untracked",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStatus {
    pub state: FileState,
    /// Lines added and removed since the last commit, when known.
    pub lines: Option<LineCounts>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineCounts {
    pub added: u64,
    pub removed: u64,
}

/// A version control system a plugin brings, such as Git, as other plugins see it.
pub trait VersionControl {
    /// E.g. `Git`.
    fn name(&self) -> &'static str;

    /// What it calls moving a file it keeps, e.g. `git mv`.
    fn move_command(&self) -> &'static str;

    /// Whether `path` is in a working tree it keeps.
    fn controls(&self, editor: &Editor, path: &Path) -> bool;

    /// How `path` differs from the last commit; `None` when it does not, or is not known
    /// yet.
    fn status(&self, editor: &Editor, path: &Path) -> Option<FileStatus>;

    /// Records that files it keeps were moved, from and to, with what was staged of them,
    /// as if they had been moved with its move command.
    fn record_moves(&self, editor: &mut Editor, moves: &[(PathBuf, PathBuf)]);
}
