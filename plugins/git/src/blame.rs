//! Who last changed each line of a file, and in which commit, like Emacs vc-annotate: the
//! file's text in a buffer of its own, each run of lines from one commit headed in the
//! gutter by the commit's id, author and age.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use remacs_core::{Buffer, CommandResult, Editor, LineExtra, TextControls};
use remacs_text::{Format, Text};

use crate::command::{git, output, output_with_input};
use crate::history_view::ago;
use crate::repositories::working_tree;
use crate::settings::GitSettings;
use crate::status_view::SHORT_ID;
use crate::{commit_view, reads};

pub const PREVIOUS: &str = "git-blame-previous";
pub const OPEN_COMMIT: &str = "git-blame-open-commit";
const NOT_A_BLAME_BUFFER: &str = "Not a blame buffer";
const HINTS: &[(&str, &str)] = &[("↵", "Commit"), ("b", "Before it"), ("q", "Back")];
/// Of the longest age, `11 months ago`.
const AGE_WIDTH: usize = 13;

#[derive(Debug, PartialEq, Eq)]
pub struct BlameCommit {
    pub id: String,
    pub author: String,
    /// Seconds since the epoch.
    pub time: i64,
    /// The file's path in the commit, relative to the root.
    pub path: PathBuf,
    /// The commit before it that had the file, and the file's path there.
    pub previous: Option<(String, PathBuf)>,
}

impl BlameCommit {
    /// The lines changed and not yet committed.
    fn is_uncommitted(&self) -> bool {
        self.id.bytes().all(|byte| byte == b'0')
    }
}

/// What `git blame --porcelain` printed: each line's commit, by its place in `commits`,
/// and the line's number in that commit, from 1.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Blame {
    pub commits: Vec<BlameCommit>,
    pub lines: Vec<(usize, usize)>,
    /// The file's bytes as the commits hold them, each line ended by an LF.
    pub text: Vec<u8>,
}

/// The state of a blame buffer.
pub struct BlameView {
    root: PathBuf,
    /// Relative to the root.
    path: PathBuf,
    /// The commit blamed; none for the visited file's text.
    revision: Option<String>,
    /// Of the file blamed, to read its older versions as it was read.
    format: Format,
    blame: Blame,
    author_width: usize,
}

impl BlameView {
    fn commit_at(&self, line: usize) -> Option<&BlameCommit> {
        let &(commit, _) = self.blame.lines.get(line)?;
        Some(&self.blame.commits[commit])
    }

    fn annotation_width(&self) -> usize {
        SHORT_ID + 1 + self.author_width + 1 + AGE_WIDTH
    }
}

pub fn parse_blame(printed: &[u8]) -> Blame {
    let mut blame = Blame::default();
    let mut indexes = HashMap::new();
    let mut current = None;
    for line in printed.split(|&byte| byte == b'\n') {
        if let Some(code) = line.strip_prefix(b"\t") {
            let Some((commit, original)) = current else {
                continue;
            };
            blame.lines.push((commit, original));
            blame.text.extend_from_slice(code);
            blame.text.push(b'\n');
            continue;
        }
        let line = String::from_utf8_lossy(line);
        let (key, value) = line.split_once(' ').unwrap_or((&line, ""));
        let commit = current.map(|(commit, _)| commit);
        match key {
            "author" => set(&mut blame, commit, |commit| commit.author = value.into()),
            "author-time" => set(&mut blame, commit, |commit| {
                commit.time = value.parse().unwrap_or(0);
            }),
            "filename" => set(&mut blame, commit, |commit| commit.path = value.into()),
            "previous" => set(&mut blame, commit, |commit| {
                commit.previous =
                    value.split_once(' ').map(|(id, path)| (id.to_owned(), path.into()));
            }),
            id if id.len() >= 40 && id.bytes().all(|byte| byte.is_ascii_hexdigit()) => {
                let original =
                    value.split(' ').next().and_then(|number| number.parse().ok()).unwrap_or(1);
                let index = match indexes.get(id) {
                    Some(&index) => index,
                    None => {
                        blame.commits.push(BlameCommit {
                            id: id.to_owned(),
                            author: String::new(),
                            time: 0,
                            path: PathBuf::new(),
                            previous: None,
                        });
                        indexes.insert(id.to_owned(), blame.commits.len() - 1);
                        blame.commits.len() - 1
                    }
                };
                current = Some((index, original));
            }
            _ => {}
        }
    }
    blame
}

fn set(blame: &mut Blame, commit: Option<usize>, change: impl FnOnce(&mut BlameCommit)) {
    if let Some(commit) = commit {
        change(&mut blame.commits[commit]);
    }
}

/// `git blame --porcelain` with paths printed as they are, not C-quoted, so that a
/// non-ASCII path can be blamed again.
fn blame_command(root: &Path) -> Command {
    let mut command = git(root);
    command.args(["-c", "core.quotePath=false", "blame", "--porcelain"]);
    command
}

/// Blames the visited file's text, unsaved edits included, like Emacs `vc-annotate`.
pub fn blame(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    let path = buffer.path().ok_or("Not visiting a file")?.to_path_buf();
    let root = path.parent().and_then(working_tree).ok_or("Not in a Git working tree")?;
    let relative = path.strip_prefix(&root).map_err(|_| "Not in a Git working tree")?.to_path_buf();
    let snapshot = buffer.text().snapshot();
    let format = snapshot.format();
    if format.binary {
        return Err("Binary file".into());
    }
    let line = buffer.point_line();
    let read_root = root.clone();
    let read_path = relative.clone();
    reads::spawn(
        editor,
        format!("Blaming {}…", relative.display()),
        move || {
            // As saving would write it, so that lines in CR LF or another encoding match
            // the commits'.
            let mut contents = Vec::new();
            format
                .write(snapshot.chunks(0..snapshot.len()), &mut contents)
                .map_err(|error| error.to_string())?;
            let mut command = blame_command(&read_root);
            command.args(["--contents", "-", "--"]).arg(&read_path);
            output_with_input(&mut command, &contents)
                .map(|printed| parse_blame(&printed))
                .map_err(|error| format!("git blame: {error}"))
        },
        move |editor, blame| {
            show(editor, root, relative, None, format, blame, line);
            Ok(())
        },
    );
    Ok(())
}

/// Blames the file as it was before the commit that last changed the line at point, at
/// that line, like Emacs `vc-annotate-revision-previous-to-line`.
pub fn blame_previous(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    let view = buffer.local::<BlameView>().ok_or(NOT_A_BLAME_BUFFER)?;
    let point_line = buffer.point_line();
    let commit = view.commit_at(point_line).ok_or("Nothing here")?;
    let (id, path) =
        commit.previous.clone().ok_or("The line is as the file's first commit made it")?;
    let line = view.blame.lines[point_line].1.saturating_sub(1);
    let (root, format) = (view.root.clone(), view.format);
    let read_root = root.clone();
    let (read_id, read_path) = (id.clone(), path.clone());
    reads::spawn(
        editor,
        format!("Blaming {} at {}…", path.display(), short(&id)),
        move || {
            output(blame_command(&read_root).args([&read_id, "--"]).arg(&read_path))
                .map(|printed| parse_blame(&printed))
                .map_err(|error| format!("git blame: {error}"))
        },
        move |editor, blame| {
            show(editor, root, path, Some(id), format, blame, line);
            Ok(())
        },
    );
    Ok(())
}

/// Shows the commit that last changed the line at point, at the file.
pub fn open_commit(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    let view = buffer.local::<BlameView>().ok_or(NOT_A_BLAME_BUFFER)?;
    let commit = view.commit_at(buffer.point_line()).ok_or("Nothing here")?;
    if commit.is_uncommitted() {
        return Err("Not committed yet".into());
    }
    let (root, id, path) = (view.root.clone(), commit.id.clone(), commit.path.clone());
    commit_view::open(editor, &root, &id, Some(&path))
}

/// Shows `blame` of `path`, relative to `root`, at `revision`, none for the visited file's
/// text, in its buffer, made again if it is shown already, point on `line`.
fn show(
    editor: &mut Editor,
    root: PathBuf,
    path: PathBuf,
    revision: Option<String>,
    format: Format,
    mut blame: Blame,
    line: usize,
) {
    let name = match &revision {
        Some(id) => format!("{} @ {} (blame)", path.display(), short(id)),
        None => format!("{} (blame)", path.display()),
    };
    let mode = editor.mode_for(Some(&root.join(&path)));
    let directory = root.join(&path).parent().unwrap_or(&root).to_path_buf();
    let bytes = std::mem::take(&mut blame.text);
    let text = Text::from(String::from_utf8_lossy(&format.decode(&bytes)).as_ref());
    // Another working tree's file of the same path gets a buffer of its own.
    let shown = editor
        .buffers()
        .iter()
        .find(|buffer| {
            buffer.local::<BlameView>().is_some_and(|view| {
                view.root == root && view.path == path && view.revision == revision
            })
        })
        .map(|buffer| buffer.name().to_owned());
    let view = BlameView {
        author_width: editor.plugin_state::<GitSettings>().blame_author_width,
        root,
        path,
        revision,
        format,
        blame,
    };
    if shown.is_some_and(|shown| editor.switch_to_buffer(&shown)) {
        let buffer = editor.window_buffer_mut();
        buffer.replace_text(text);
        buffer.set_local(view);
    } else {
        let mut buffer = Buffer::new(name, text, mode).with_directory(directory).read_only();
        buffer.set_local(view);
        editor.add_buffer(buffer);
    }
    editor.set_point_at(line, 0);
}

/// The commit, author and age at the first line of each run of lines from one commit.
pub fn line_extras(editor: &Editor, lines: Range<usize>) -> Vec<LineExtra> {
    let Some(view) = editor.window_buffer().local::<BlameView>() else {
        return Vec::new();
    };
    let now =
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64);
    lines
        .filter_map(|line| {
            let &(commit, _) = view.blame.lines.get(line)?;
            let starts_run = line == 0 || view.blame.lines[line - 1].0 != commit;
            let number = if starts_run {
                annotation(&view.blame.commits[commit], view.author_width, now)
            } else {
                String::new()
            };
            let width = view.annotation_width();
            Some(LineExtra {
                line,
                number: Some(format!("{number:<width$}")),
                ..LineExtra::default()
            })
        })
        .collect()
}

fn annotation(commit: &BlameCommit, author_width: usize, now: i64) -> String {
    if commit.is_uncommitted() {
        return format!("{:SHORT_ID$} Not committed", "");
    }
    let mut author: String = commit.author.chars().take(author_width).collect();
    if commit.author.chars().count() > author_width {
        author.pop();
        author.push('…');
    }
    format!("{} {author:<author_width$} {}", short(&commit.id), ago(now - commit.time))
}

pub fn controls(editor: &Editor) -> Option<TextControls> {
    let view = editor.window_buffer().local::<BlameView>()?;
    Some(TextControls {
        hints: HINTS,
        number_width: Some(view.annotation_width()),
        ..TextControls::default()
    })
}

/// While a prompt is open, keys go to it, not to the blame buffer under it.
pub fn shown(editor: &Editor) -> bool {
    editor.buffer().local::<BlameView>().is_some()
}

fn short(id: &str) -> String {
    id.chars().take(SHORT_ID).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_gives_each_line_its_commit_and_its_number_there() {
        let printed = b"\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 1 1 2
author Ann
author-time 100
previous bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb old.rs
filename lib.rs
\tone
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 2 2
\ttwo
cccccccccccccccccccccccccccccccccccccccc 7 3 1
author Bob
author-time 50
boundary
filename lib.rs
\t\tthree
";
        let blame = parse_blame(printed);
        assert_eq!(blame.text, b"one\ntwo\n\tthree\n");
        assert_eq!(blame.lines, [(0, 1), (0, 2), (1, 7)]);
        assert_eq!(blame.commits[0].author, "Ann");
        assert_eq!(blame.commits[0].time, 100);
        assert_eq!(blame.commits[0].previous, Some(("b".repeat(40), PathBuf::from("old.rs"))));
        assert_eq!(blame.commits[1].path, PathBuf::from("lib.rs"));
        assert_eq!(blame.commits[1].previous, None);
    }

    #[test]
    fn long_authors_are_cut_to_the_column() {
        let commit = BlameCommit {
            id: "1234567890".repeat(4),
            author: "Alexandra Longname".into(),
            time: 0,
            path: PathBuf::new(),
            previous: None,
        };
        assert_eq!(annotation(&commit, 8, 0), "1234567 Alexand… just now");
    }
}
