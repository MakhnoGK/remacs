//! One file's `git diff`, and patches of single hunks for `git apply`.

/// A file's diff: the header Git needs to apply any of it, then the hunks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDiff {
    /// From `diff --git` to the first hunk, each line ending in a newline.
    pub header: String,
    pub hunks: Vec<Hunk>,
    /// Git compared the file as binary, so there are no hunks.
    pub binary: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// `@@ -39,3 +39,6 @@ pub fn insert`
    pub header: String,
    /// With their `+`, `-`, ` ` or `\` first.
    pub lines: Vec<String>,
    /// The line of the new file the hunk starts at, from 1.
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
                new_start: new_start(line).unwrap_or(1),
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

/// The `39` of `@@ -39,3 +39,6 @@`.
fn new_start(header: &str) -> Option<usize> {
    let added = header.split(' ').find(|part| part.starts_with('+'))?;
    added[1..].split(',').next()?.parse().ok()
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
        assert_eq!(diff.hunks[1].new_start, 21);
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
    fn binary_files_have_no_hunks() {
        let diff =
            parse_diff("diff --git a/i.png b/i.png\nBinary files a/i.png and b/i.png differ\n")
                .unwrap();
        assert!(diff.binary && diff.hunks.is_empty());
    }
}
