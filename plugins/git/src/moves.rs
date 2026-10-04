//! Telling Git of files already moved, as `git mv` would have: what was staged at the old
//! path is staged at the new one, and unstaged changes stay unstaged.

use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::command::{git, output, output_with_input};

/// Moves the index entries at or under each `from` to `to`; paths are relative to `root`.
pub fn record(root: &Path, moves: &[(PathBuf, PathBuf)]) -> io::Result<()> {
    let staged = output(
        git(root)
            .args(["ls-files", "--stage", "-z", "--"])
            .args(moves.iter().map(|(from, _)| from)),
    )?;
    let input = index_info(&staged, moves);
    if !input.is_empty() {
        output_with_input(
            git(root).args(["update-index", "-z", "--index-info"]),
            &input,
        )?;
    }
    Ok(())
}

/// `git update-index -z --index-info` input that removes the entries `git ls-files
/// --stage -z` listed under a move's source and adds them under its destination. Removals
/// come first, so that names passed on or swapped end up added.
fn index_info(staged: &[u8], moves: &[(PathBuf, PathBuf)]) -> Vec<u8> {
    let (mut removals, mut additions) = (Vec::new(), Vec::new());
    for record in staged.split(|&byte| byte == 0) {
        let Some(tab) = record.iter().position(|&byte| byte == b'\t') else {
            continue;
        };
        let (info, path) = (
            &record[..tab],
            Path::new(OsStr::from_bytes(&record[tab + 1..])),
        );
        let mut fields = info.split(|&byte| byte == b' ');
        let (Some(mode), Some(object), Some(b"0")) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let Some(moved) = moves.iter().find_map(|(from, to)| {
            let rest = path.strip_prefix(from).ok()?;
            Some(if rest.as_os_str().is_empty() {
                to.clone()
            } else {
                to.join(rest)
            })
        }) else {
            continue;
        };
        let zeros = vec![b'0'; object.len()];
        removals.push(entry(b"0", &zeros, path));
        additions.push(entry(mode, object, &moved));
    }
    removals.into_iter().chain(additions).flatten().collect()
}

/// `mode object\tpath\0`
fn entry(mode: &[u8], object: &[u8], path: &Path) -> Vec<u8> {
    [
        mode,
        b" ",
        object,
        b"\t",
        path.as_os_str().as_bytes(),
        b"\0",
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_move_with_files_and_directories() {
        let staged = b"100644 aaaa 0\tsrc/a.rs\0\
100755 bbbb 0\tsrc/dir/run.sh\0\
100644 cccc 0\tsrc/dir/deep/c.rs\0\
100644 dddd 0\tother.rs\0";
        let moves = [
            (PathBuf::from("src/a.rs"), PathBuf::from("src/b.rs")),
            (PathBuf::from("src/dir"), PathBuf::from("lib")),
        ];
        let input = String::from_utf8(index_info(staged, &moves)).unwrap();
        let lines: Vec<&str> = input.split_terminator('\0').collect();
        assert_eq!(
            lines,
            [
                "0 0000\tsrc/a.rs",
                "0 0000\tsrc/dir/run.sh",
                "0 0000\tsrc/dir/deep/c.rs",
                "100644 aaaa\tsrc/b.rs",
                "100755 bbbb\tlib/run.sh",
                "100644 cccc\tlib/deep/c.rs",
            ]
        );
    }

    #[test]
    fn conflicted_entries_are_left_alone() {
        let staged = b"100644 aaaa 1\tc.rs\x00100644 bbbb 2\tc.rs\0";
        let moves = [(PathBuf::from("c.rs"), PathBuf::from("d.rs"))];
        assert!(index_info(staged, &moves).is_empty());
    }
}
