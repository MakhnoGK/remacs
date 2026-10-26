//! One file's `git diff`, and patches of single hunks for `git apply`.

use std::io::{self, BufRead};
use std::mem;
use std::path::PathBuf;

use remacs_core::{FileState, LineCounts};

/// A file's diff: the header Git needs to apply any of it, then the hunks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDiff {
    /// From `diff --git` to the first hunk, each line ending in a newline.
    pub header: String,
    pub hunks: Vec<Hunk>,
    /// Git compared the file as binary, so there are no hunks.
    pub binary: bool,
    /// The file's size, when it was too large to compare; in a commit, the diff's, when
    /// it was too large to keep.
    pub too_large: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// `@@ -39,3 +39,6 @@ pub fn insert`
    pub header: String,
    /// With their `+`, `-`, ` ` or `\` first.
    pub lines: Vec<String>,
    /// The lines of the old and new file the hunk starts at, from 1.
    pub old_start: usize,
    pub new_start: usize,
}

impl Hunk {
    /// The line of the new file that `line` of the hunk is at, or follows when removed.
    pub fn new_line(&self, line: usize) -> usize {
        let before = self.lines[..line.min(self.lines.len())]
            .iter()
            .filter(|text| !text.starts_with(['-', '\\']))
            .count();
        self.new_start + before
    }
}

impl FileDiff {
    /// What the diff did to the file, as its header says.
    pub fn state(&self) -> FileState {
        match () {
            _ if self.header.contains("\nnew file mode") => FileState::Added,
            _ if self.header.contains("\ndeleted file mode") => FileState::Deleted,
            _ if self.original().is_some() => FileState::Renamed,
            _ => FileState::Modified,
        }
    }

    /// Where a renamed file came from.
    pub fn original(&self) -> Option<PathBuf> {
        self.header
            .lines()
            .find_map(|line| line.strip_prefix("rename from "))
            .map(PathBuf::from)
    }

    /// Lines added and removed; none for a binary file.
    pub fn line_counts(&self) -> Option<LineCounts> {
        if self.binary || self.too_large.is_some() {
            return None;
        }
        let count = |sign: char| {
            let lines = self.hunks.iter().flat_map(|hunk| &hunk.lines);
            lines.filter(|line| line.starts_with(sign)).count() as u64
        };
        Some(LineCounts {
            added: count('+'),
            removed: count('-'),
        })
    }

    /// What is said in place of the hunks when there are none.
    pub fn summary(&self) -> String {
        match self.too_large {
            Some(bytes) => format!("Large file, {}, not shown", size(bytes)),
            None if self.binary => "Binary file".into(),
            None if self.header.contains("\nnew file mode") => "Empty file".into(),
            None => "No changes to show".into(),
        }
    }

    /// A patch of hunk `index` alone, for `git apply`.
    pub fn patch(&self, index: usize) -> Option<String> {
        let hunk = self.hunks.get(index)?;
        let mut patch = self.header.clone();
        patch.push_str(&hunk.header);
        patch.push('\n');
        for line in &hunk.lines {
            patch.push_str(line);
            patch.push('\n');
        }
        Some(patch)
    }
}

/// `840 B`, `12.5 KB`, `3.2 MB`.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Reads `git diff` of one file; `None` when it printed nothing.
pub fn parse_diff(output: &str) -> Option<FileDiff> {
    if output.is_empty() {
        return None;
    }
    let mut diff = FileDiff::default();
    for line in output.lines() {
        if line.starts_with("@@") {
            diff.hunks.push(Hunk {
                header: line.to_owned(),
                lines: Vec::new(),
                old_start: start(line, '-').unwrap_or(1),
                new_start: start(line, '+').unwrap_or(1),
            });
        } else if let Some(hunk) = diff.hunks.last_mut() {
            hunk.lines.push(line.to_owned());
        } else {
            diff.binary |= line.starts_with("Binary files ");
            diff.header.push_str(line);
            diff.header.push('\n');
        }
    }
    Some(diff)
}

/// Reads a diff of several files, Git's paths unquoted, into each file's path and diff.
pub fn split_patch(output: &str) -> Vec<(PathBuf, FileDiff)> {
    read_patch(&mut output.as_bytes(), u64::MAX).expect("reading from memory")
}

/// Like [`split_patch`], reading the diff as Git prints it. Files' diffs are kept up to
/// `max_bytes` in all; a file whose diff does not fit what is left keeps its header only,
/// the diff's size as `too_large`.
pub fn read_patch(
    printed: &mut dyn BufRead,
    max_bytes: u64,
) -> io::Result<Vec<(PathBuf, FileDiff)>> {
    let (mut files, mut left) = (Vec::new(), max_bytes);
    let mut file = PatchFile::default();
    let mut line = Vec::new();
    loop {
        line.clear();
        let ended = printed.read_until(b'\n', &mut line)? == 0;
        // A diff of the work tree tells of files with conflicts its own way.
        let starts_file = [&b"diff --git "[..], b"diff --cc ", b"* Unmerged path "]
            .iter()
            .any(|start| line.starts_with(start));
        if ended || starts_file {
            files.extend(mem::take(&mut file).finish(&mut left));
            if ended {
                return Ok(files);
            }
        }
        file.push(&line, left);
    }
}

/// A file of a patch as it is read.
#[derive(Default)]
struct PatchFile {
    text: String,
    /// The whole diff's, what was left out included.
    bytes: u64,
    /// Where the first hunk starts in `text`.
    hunks: Option<usize>,
    cut: bool,
}

impl PatchFile {
    fn push(&mut self, line: &[u8], left: u64) {
        self.bytes += line.len() as u64;
        if self.cut {
            return;
        }
        if self.hunks.is_none() && line.starts_with(b"@@") {
            self.hunks = Some(self.text.len());
        }
        if let Some(hunks) = self.hunks
            && self.bytes > left
        {
            self.text.truncate(hunks);
            self.cut = true;
            return;
        }
        self.text.push_str(&String::from_utf8_lossy(line));
    }

    fn finish(self, left: &mut u64) -> Option<(PathBuf, FileDiff)> {
        let path = patched_path(&self.text)?;
        let mut diff = parse_diff(&self.text)?;
        if self.cut {
            diff.too_large = Some(self.bytes);
        } else {
            *left = left.saturating_sub(self.bytes);
        }
        Some((path, diff))
    }
}

/// The path after `+++ b/`, or `--- a/` for a deleted file; else, for a binary file, the
/// second half of `diff --git a/<path> b/<path>`.
fn patched_path(file: &str) -> Option<PathBuf> {
    let header = file.split("\n@@").next().unwrap_or(file);
    // Git ends the name with a tab when it holds a space.
    let named = |prefix: &str| {
        header
            .lines()
            .find_map(|line| line.strip_prefix(prefix))
            .map(|name| PathBuf::from(name.strip_suffix('\t').unwrap_or(name)))
    };
    named("+++ b/").or_else(|| named("--- a/")).or_else(|| {
        let paths = header.lines().next()?.strip_prefix("diff --git a/")?;
        let path_length = paths.len().checked_sub(" b/".len())? / 2;
        paths.get(path_length + " b/".len()..).map(PathBuf::from)
    })
}

/// The `39` of `@@ -39,3 +39,6 @@`.
/// Where the side marked by `sign`, `-` old or `+` new, starts in `@@ -39,3 +39,6 @@`.
fn start(header: &str, sign: char) -> Option<usize> {
    let side = header.split(' ').find(|part| part.starts_with(sign))?;
    side[1..].split(',').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/src/buffer.rs b/src/buffer.rs
index 1111111..2222222 100644
--- a/src/buffer.rs
+++ b/src/buffer.rs
@@ -3,3 +3,4 @@ fn one
 a
-b
+B
+c
 d
@@ -20,2 +21,2 @@ fn two
-x
+y
\\ No newline at end of file
";

    #[test]
    fn hunks_follow_the_header() {
        let diff = parse_diff(DIFF).unwrap();
        assert!(
            diff.header.starts_with("diff --git") && diff.header.ends_with("+++ b/src/buffer.rs\n")
        );
        assert_eq!(diff.hunks.len(), 2);
        assert_eq!(diff.hunks[0].header, "@@ -3,3 +3,4 @@ fn one");
        assert_eq!(diff.hunks[0].lines, [" a", "-b", "+B", "+c", " d"]);
        assert_eq!((diff.hunks[1].old_start, diff.hunks[1].new_start), (20, 21));
        assert!(!diff.binary);
        assert_eq!(parse_diff(""), None);
    }

    #[test]
    fn a_hunks_patch_has_the_header_and_that_hunk_only() {
        let diff = parse_diff(DIFF).unwrap();
        let patch = diff.patch(1).unwrap();
        assert!(patch.starts_with(&diff.header));
        assert!(
            patch.ends_with("@@ -20,2 +21,2 @@ fn two\n-x\n+y\n\\ No newline at end of file\n")
        );
        assert!(!patch.contains("fn one"));
        assert_eq!(diff.patch(2), None);
    }

    #[test]
    fn lines_map_to_the_new_file() {
        let hunk = &parse_diff(DIFF).unwrap().hunks[0];
        assert_eq!(hunk.new_line(0), 3);
        assert_eq!(hunk.new_line(1), 4);
        assert_eq!(hunk.new_line(2), 4);
        assert_eq!(hunk.new_line(4), 6);
    }

    #[test]
    fn a_patch_splits_into_its_files() {
        let patch = format!(
            "{DIFF}diff --git a/gone.rs b/gone.rs\n\
deleted file mode 100644\n\
--- a/gone.rs\n\
+++ /dev/null\n\
@@ -1 +0,0 @@\n\
-x\n\
diff --git a/i m.png b/i m.png\n\
Binary files a/i m.png and b/i m.png differ\n"
        );
        let files = split_patch(&patch);
        let paths: Vec<_> = files
            .iter()
            .map(|(path, _)| path.to_str().unwrap())
            .collect();
        assert_eq!(paths, ["src/buffer.rs", "gone.rs", "i m.png"]);
        assert_eq!(files[0].1.hunks.len(), 2);
        assert_eq!(files[1].1.hunks[0].lines, ["-x"]);
        assert!(files[2].1.binary);
        assert!(split_patch("").is_empty());
    }

    #[test]
    fn a_work_tree_diff_splits_at_files_with_conflicts_and_names_with_spaces() {
        let patch = "diff --git a/a b.rs b/a b.rs\n\
--- a/a b.rs\t\n\
+++ b/a b.rs\t\n\
@@ -1 +1 @@\n\
-a\n\
+b\n\
diff --cc both.rs\n\
index 1111111,2222222..0000000\n\
--- a/both.rs\n\
+++ b/both.rs\n\
@@@ -1,1 -1,1 +1,5 @@@\n\
++<<<<<<< HEAD\n\
* Unmerged path gone.rs\n";
        let files = split_patch(patch);
        assert_eq!(files[0].0, PathBuf::from("a b.rs"));
        assert_eq!(files[0].1.hunks[0].lines, ["-a", "+b"]);
        assert_eq!(files[1].0, PathBuf::from("both.rs"));
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn a_file_whose_diff_does_not_fit_what_is_left_keeps_its_header_only() {
        let small = "diff --git a/s.rs b/s.rs\n--- a/s.rs\n+++ b/s.rs\n@@ -1 +1 @@\n-a\n+b\n";
        let patch = format!("{DIFF}{small}");
        let files = read_patch(&mut patch.as_bytes(), small.len() as u64).unwrap();
        let (big, kept) = (&files[0].1, &files[1].1);
        assert_eq!(files[0].0, PathBuf::from("src/buffer.rs"));
        assert!(big.hunks.is_empty() && big.header.ends_with("+++ b/src/buffer.rs\n"));
        assert_eq!(big.too_large, Some(DIFF.len() as u64));
        assert_eq!(kept.hunks[0].lines, ["-a", "+b"]);
        assert_eq!(kept.too_large, None);
    }

    #[test]
    fn binary_files_have_no_hunks() {
        let diff =
            parse_diff("diff --git a/i.png b/i.png\nBinary files a/i.png and b/i.png differ\n")
                .unwrap();
        assert!(diff.binary && diff.hunks.is_empty());
    }
}
