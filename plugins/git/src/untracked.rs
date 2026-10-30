//! What untracked files hold, as all added lines, and what untracked directories hold.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use crate::command::{git, output};
use crate::diff::{FileDiff, Hunk};

/// The bytes Git looks at for a NUL to take a file as binary.
const BINARY_PROBE: usize = 8000;

/// The files of an untracked directory, by path relative to the root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// The first so many, by path.
    pub files: Vec<PathBuf>,
    /// Left out past the limit.
    pub more: usize,
}

/// The untracked file at `relative` as a diff against nothing, as `git diff --no-index`
/// has it; read here, as Git would take a process per file. One larger than `max_bytes`
/// is not read.
pub fn diff(root: &Path, relative: &Path, max_bytes: u64) -> io::Result<FileDiff> {
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.len() > max_bytes {
        return Ok(FileDiff { too_large: Some(metadata.len()), ..FileDiff::default() });
    }
    // A link's contents, to Git, are its target; a pipe made since Git listed it would
    // never end.
    let bytes = match metadata.file_type() {
        kind if kind.is_symlink() => fs::read_link(&path)?.into_os_string().into_vec(),
        kind if kind.is_file() => fs::read(&path)?,
        _ => return Ok(FileDiff::default()),
    };
    let name = relative.display();
    let mut diff = FileDiff {
        header: format!("diff --git a/{name} b/{name}\nnew file mode 100644\n"),
        ..FileDiff::default()
    };
    // Git's own test of a binary file.
    diff.binary = bytes[..bytes.len().min(BINARY_PROBE)].contains(&0);
    if diff.binary || bytes.is_empty() {
        return Ok(diff);
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<String> = text.lines().map(|line| format!("+{line}")).collect();
    let header = match lines.len() {
        1 => "@@ -0,0 +1 @@".to_owned(),
        count => format!("@@ -0,0 +1,{count} @@"),
    };
    if !text.ends_with('\n') {
        lines.push("\\ No newline at end of file".into());
    }
    diff.hunks.push(Hunk { header, lines, old_start: 0, new_start: 1 });
    Ok(diff)
}

/// The files Git would list as untracked under `directory`, the first `max_files` of them.
pub fn list(root: &Path, directory: &Path, max_files: usize) -> io::Result<Listing> {
    let printed = output(
        git(root).args(["ls-files", "-z", "--others", "--exclude-standard", "--"]).arg(directory),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untracked_file_reads_as_git_diff_no_index_has_it() {
        let root = std::env::temp_dir().join(format!("remacs-untracked-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let files = [("two.rs", "a\nb"), ("empty.rs", ""), ("one.rs", "x\n")];
        for (name, text) in files {
            fs::write(root.join(name), text).unwrap();
        }
        fs::write(root.join("data.bin"), b"\0\x01").unwrap();
        let read = |name: &str| diff(&root, Path::new(name), 1024).unwrap();

        let two = read("two.rs");
        assert_eq!(two.hunks[0].header, "@@ -0,0 +1,2 @@");
        assert_eq!(two.hunks[0].lines, ["+a", "+b", "\\ No newline at end of file"]);
        assert_eq!(read("one.rs").hunks[0].header, "@@ -0,0 +1 @@");
        assert_eq!(read("empty.rs").summary(), "Empty file");
        assert_eq!(read("data.bin").summary(), "Binary file");
        let cut = diff(&root, Path::new("two.rs"), 2).unwrap();
        assert_eq!((cut.too_large, cut.hunks.len()), (Some(3), 0));
        fs::remove_dir_all(&root).unwrap();
    }
}
