//! What untracked files hold, as all added lines, and what untracked directories hold.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::command::{diff_output, git, output};
use crate::diff::{FileDiff, parse_diff};

/// The files of an untracked directory, by path relative to the root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// The first so many, by path.
    pub files: Vec<PathBuf>,
    /// Left out past the limit.
    pub more: usize,
}

/// The untracked file at `relative` as a diff against nothing; one larger than
/// `max_bytes` is not read.
pub fn diff(root: &Path, relative: &Path, max_bytes: u64) -> io::Result<FileDiff> {
    let bytes = fs::symlink_metadata(root.join(relative))?.len();
    if bytes > max_bytes {
        return Ok(FileDiff {
            too_large: Some(bytes),
            ..FileDiff::default()
        });
    }
    let printed = diff_output(
        git(root)
            .args(["diff", "--no-index", "--no-color", "--no-ext-diff", "--"])
            .arg("/dev/null")
            .arg(relative),
    )?;
    Ok(parse_diff(&String::from_utf8_lossy(&printed)).unwrap_or_default())
}

/// The files Git would list as untracked under `directory`, the first `max_files` of them.
pub fn list(root: &Path, directory: &Path, max_files: usize) -> io::Result<Listing> {
    let printed = output(
        git(root)
            .args(["ls-files", "-z", "--others", "--exclude-standard", "--"])
            .arg(directory),
    )?;
    let mut files: Vec<PathBuf> = printed
        .split(|&byte| byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(OsStr::from_bytes(path)))
        .collect();
    files.sort();
    let more = files.len().saturating_sub(max_files);
    files.truncate(max_files);
    Ok(Listing { files, more })
}
