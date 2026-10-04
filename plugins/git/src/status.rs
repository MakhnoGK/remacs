//! What `git status --porcelain=v2` and `git diff --numstat` say of a working tree.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use remacs_core::{FileState, FileStatus, LineCounts};

use crate::details::Details;
use crate::diff::FileDiff;
use crate::history::Log;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub head: Head,
    pub upstream: Option<Upstream>,
    pub stashes: usize,
    /// By path relative to the root.
    pub files: HashMap<PathBuf, FileEntry>,
    /// Relative, listed whole rather than file by file.
    pub untracked_directories: Vec<PathBuf>,
    /// Read while a status buffer shows the tree.
    pub details: Option<Details>,
    /// Of the files a status buffer shows unfolded.
    pub diffs: HashMap<DiffKey, FileDiff>,
    /// Read while a status buffer shows the tree; shared, as it rarely changes.
    pub history: Option<Arc<Log>>,
}

/// One side of a file's changes: staged, or not yet.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct DiffKey {
    pub staged: bool,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    Branch(String),
    /// At a commit, by its id.
    Detached(String),
    /// On a branch with no commits yet.
    Unborn(String),
}

impl Default for Head {
    fn default() -> Self {
        Head::Unborn(String::new())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upstream {
    pub name: String,
    /// Commits ahead of it and behind it; `None` when it is gone.
    pub ahead_behind: Option<(u64, u64)>,
}

/// One changed path: the `XY` of `git status --short`, staged then unstaged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub staged: u8,
    pub unstaged: u8,
    pub conflicted: bool,
    /// Where a renamed or copied file came from.
    pub original: Option<PathBuf>,
    /// Since the last commit.
    pub lines: Option<LineCounts>,
    /// Read with the details: what is staged, and what is not yet.
    pub staged_lines: Option<LineCounts>,
    pub unstaged_lines: Option<LineCounts>,
}

/// Which of a file's line counts a `git diff --numstat` gives.
#[derive(Clone, Copy)]
pub enum Counted {
    SinceCommit,
    Staged,
    Unstaged,
}

impl FileEntry {
    pub fn state(&self) -> FileState {
        match (self.staged, self.unstaged) {
            _ if self.conflicted => FileState::Conflicted,
            (b'?', _) => FileState::Untracked,
            (b'A' | b'C', _) => FileState::Added,
            (b'R', _) => FileState::Renamed,
            (b'D', _) | (_, b'D') => FileState::Deleted,
            _ => FileState::Modified,
        }
    }
}

impl Snapshot {
    /// How the file or directory at `relative` differs from the last commit.
    pub fn status(&self, relative: &Path) -> Option<FileStatus> {
        if let Some(entry) = self.files.get(relative) {
            return Some(FileStatus {
                state: entry.state(),
                lines: entry.lines,
            });
        }
        self.untracked_directories
            .iter()
            .any(|directory| relative.starts_with(directory))
            .then_some(FileStatus {
                state: FileState::Untracked,
                lines: None,
            })
    }

    pub fn is_unborn(&self) -> bool {
        matches!(self.head, Head::Unborn(_))
    }

    /// Adds the line counts of `git diff --numstat -z`.
    pub fn add_line_counts(&mut self, numstat: &[u8], counted: Counted) {
        for (path, lines) in parse_numstat(numstat) {
            if let Some(entry) = self.files.get_mut(&path) {
                *match counted {
                    Counted::SinceCommit => &mut entry.lines,
                    Counted::Staged => &mut entry.staged_lines,
                    Counted::Unstaged => &mut entry.unstaged_lines,
                } = lines;
            }
        }
    }
}

/// Reads `git status --porcelain=v2 -z --branch --show-stash`.
pub fn parse_status(output: &[u8]) -> Snapshot {
    let mut snapshot = Snapshot::default();
    let (mut oid, mut branch) = (String::new(), None);
    let mut records = output.split(|&byte| byte == 0);
    while let Some(record) = records.next() {
        let Some((&kind, rest)) = record.split_first() else {
            continue;
        };
        let rest = rest.strip_prefix(b" ").unwrap_or(rest);
        match kind {
            b'#' => {
                let header = String::from_utf8_lossy(rest);
                let (key, value) = header.split_once(' ').unwrap_or((&header, ""));
                match key {
                    "branch.oid" => oid = value.to_owned(),
                    "branch.head" => branch = Some(value.to_owned()),
                    "branch.upstream" => {
                        snapshot.upstream = Some(Upstream {
                            name: value.to_owned(),
                            ahead_behind: None,
                        });
                    }
                    "branch.ab" => {
                        if let Some(upstream) = &mut snapshot.upstream {
                            upstream.ahead_behind = parse_ahead_behind(value);
                        }
                    }
                    "stash" => snapshot.stashes = value.parse().unwrap_or(0),
                    _ => {}
                }
            }
            b'1' => insert_change(&mut snapshot, rest, 7, None),
            b'2' => {
                let original = records.next().map(path);
                insert_change(&mut snapshot, rest, 8, original);
            }
            b'u' => {
                if let Some((xy, relative)) = fields(rest, 9) {
                    snapshot.files.insert(relative, entry(xy, true, None));
                }
            }
            b'?' => {
                let relative = path(rest);
                if rest.ends_with(b"/") {
                    snapshot.untracked_directories.push(relative);
                } else {
                    snapshot.files.insert(relative, entry(b"??", false, None));
                }
            }
            _ => {}
        }
    }
    snapshot.head = match branch.as_deref() {
        Some("(detached)") | None => Head::Detached(oid),
        Some(name) if oid == "(initial)" => Head::Unborn(name.to_owned()),
        Some(name) => Head::Branch(name.to_owned()),
    };
    snapshot
}

fn insert_change(
    snapshot: &mut Snapshot,
    rest: &[u8],
    before_path: usize,
    original: Option<PathBuf>,
) {
    if let Some((xy, relative)) = fields(rest, before_path) {
        snapshot.files.insert(relative, entry(xy, false, original));
    }
}

/// The `XY` field and the path that follows `before_path` space-separated fields, `XY`
/// the first of them.
fn fields(record: &[u8], before_path: usize) -> Option<(&[u8], PathBuf)> {
    let mut parts = record.splitn(before_path + 1, |&byte| byte == b' ');
    let xy = parts.next()?;
    let relative = parts.nth(before_path - 1)?;
    (xy.len() == 2).then(|| (xy, path(relative)))
}

fn entry(xy: &[u8], conflicted: bool, original: Option<PathBuf>) -> FileEntry {
    FileEntry {
        staged: xy[0],
        unstaged: xy[1],
        conflicted,
        original,
        lines: None,
        staged_lines: None,
        unstaged_lines: None,
    }
}

/// `+2 -0`
fn parse_ahead_behind(value: &str) -> Option<(u64, u64)> {
    let (ahead, behind) = value.split_once(' ')?;
    Some((
        ahead.strip_prefix('+')?.parse().ok()?,
        behind.strip_prefix('-')?.parse().ok()?,
    ))
}

/// Paths and their added and removed lines, `None` for a binary file; a renamed file by
/// its new path.
fn parse_numstat(output: &[u8]) -> Vec<(PathBuf, Option<LineCounts>)> {
    let mut counts = Vec::new();
    let mut records = output.split(|&byte| byte == 0);
    while let Some(record) = records.next() {
        let mut parts = record.splitn(3, |&byte| byte == b'\t');
        let (Some(added), Some(removed), Some(relative)) =
            (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let relative = if relative.is_empty() {
            match (records.next(), records.next()) {
                (Some(_), Some(renamed)) => renamed,
                _ => break,
            }
        } else {
            relative
        };
        let number = |field: &[u8]| std::str::from_utf8(field).ok()?.parse().ok();
        let lines = number(added)
            .zip(number(removed))
            .map(|(added, removed)| LineCounts { added, removed });
        counts.push((path(relative), lines));
    }
    counts
}

fn path(bytes: &[u8]) -> PathBuf {
    let bytes = bytes.strip_suffix(b"/").unwrap_or(bytes);
    PathBuf::from(OsStr::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &[u8] = b"# branch.oid 9f3c2a1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0\
# branch.head feature/mark-ring\0\
# branch.upstream origin/feature/mark-ring\0\
# branch.ab +2 -0\0\
# stash 1\0\
1 A. N... 000000 100644 100644 0000 1111 src/mark_ring.rs\0\
1 .M N... 100644 100644 100644 2222 2222 src/buffer with space.rs\0\
2 R. N... 100644 100644 100644 3333 3333 R100 src/new.rs\0src/old.rs\0\
u UU N... 100644 100644 100644 100644 4444 5555 6666 src/conflict.rs\0\
? notes/\0\
? ring-design.md\0";

    #[test]
    fn status_lists_head_upstream_and_files() {
        let snapshot = parse_status(STATUS);
        assert_eq!(snapshot.head, Head::Branch("feature/mark-ring".into()));
        assert_eq!(
            snapshot.upstream,
            Some(Upstream {
                name: "origin/feature/mark-ring".into(),
                ahead_behind: Some((2, 0)),
            })
        );
        assert_eq!(snapshot.stashes, 1);
        let state = |path: &str| snapshot.status(Path::new(path)).map(|status| status.state);
        assert_eq!(state("src/mark_ring.rs"), Some(FileState::Added));
        assert_eq!(state("src/buffer with space.rs"), Some(FileState::Modified));
        assert_eq!(state("src/new.rs"), Some(FileState::Renamed));
        assert_eq!(state("src/conflict.rs"), Some(FileState::Conflicted));
        assert_eq!(state("ring-design.md"), Some(FileState::Untracked));
        assert_eq!(state("notes"), Some(FileState::Untracked));
        assert_eq!(state("notes/deep/draft.md"), Some(FileState::Untracked));
        assert_eq!(state("src/lib.rs"), None);
        assert_eq!(
            snapshot.files[Path::new("src/new.rs")].original,
            Some(PathBuf::from("src/old.rs"))
        );
    }

    #[test]
    fn detached_and_unborn_heads() {
        let detached = parse_status(b"# branch.oid abc123\0# branch.head (detached)\0");
        assert_eq!(detached.head, Head::Detached("abc123".into()));
        let unborn = parse_status(b"# branch.oid (initial)\0# branch.head main\0");
        assert_eq!(unborn.head, Head::Unborn("main".into()));
        assert!(unborn.upstream.is_none());
    }

    #[test]
    fn upstream_that_is_gone_has_no_counts() {
        let snapshot = parse_status(
            b"# branch.oid abc\0# branch.head topic\0# branch.upstream origin/topic\0",
        );
        assert_eq!(snapshot.upstream.unwrap().ahead_behind, None);
    }

    #[test]
    fn line_counts_go_to_their_files_renames_by_new_path() {
        let mut snapshot = parse_status(STATUS);
        snapshot.add_line_counts(
            b"88\t0\tsrc/mark_ring.rs\0\
4\t1\tsrc/buffer with space.rs\0\
2\t2\t\0src/old.rs\0src/new.rs\0\
-\t-\tsrc/conflict.rs\0",
            Counted::SinceCommit,
        );
        let lines = |path: &str| snapshot.files[Path::new(path)].lines;
        let counts = |added, removed| Some(LineCounts { added, removed });
        assert_eq!(lines("src/mark_ring.rs"), counts(88, 0));
        assert_eq!(lines("src/buffer with space.rs"), counts(4, 1));
        assert_eq!(lines("src/new.rs"), counts(2, 2));
        assert_eq!(lines("src/conflict.rs"), None);
    }
}
