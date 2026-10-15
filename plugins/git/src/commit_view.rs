//! What one commit changed, like Emacs magit-revision: who made it and when, its message,
//! then its files, each unfolding into its diff.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::hash::Hash;
use std::ops::Range;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use remacs_core::{
    Buffer, Button, CommandResult, DiffRow, Editor, Fact, FactValue, FileState, Outline,
    OutlineRow, Tone,
};
use remacs_text::Text;

use crate::command::{git, output};
use crate::diff::{FileDiff, split_patch};
use crate::history_view::ago;
use crate::status_view::{SHORT_ID, directory_label, hunk_row, push_diff};

pub const COMMIT_MODE: &str = "Git Commit";
pub const TOGGLE: &str = "git-commit-toggle";
pub const VISIT: &str = "git-commit-visit";
const NOT_A_COMMIT_BUFFER: &str = "Not a commit buffer";
const HINTS: &[(&str, &str)] = &[
    ("TAB", "Fold"),
    ("↵", "Visit file"),
    ("o", "File at commit"),
    ("q", "Back"),
];
/// Its fields end in NULs, the last before the patch.
const FORMAT: &str = "--format=%H%x00%P%x00%an%x00%at%x00%ad%x00%B%x00";
const FIELDS: usize = 6;
const DATE: &str = "--date=format:%Y-%m-%d %H:%M";

/// A commit as `git show` tells it, its changes against its first parent.
pub struct Commit {
    pub id: String,
    /// With their subjects.
    pub parents: Vec<(String, String)>,
    pub author: String,
    /// Seconds since the epoch.
    pub time: i64,
    /// As the author's clock read, e.g. `2026-10-14 18:45`.
    pub date: String,
    pub subject: String,
    /// The message after the subject, blank lines around it left out.
    pub body: Vec<String>,
    pub files: Vec<(PathBuf, FileDiff)>,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Part {
    Message,
    Files,
}

/// What a line of the commit buffer stands for; files by their place in the commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitLine {
    Heading(Part),
    Message(usize),
    File(usize),
    Diff {
        file: usize,
        hunk: usize,
        line: Option<usize>,
    },
    NoDiff(usize),
}

impl CommitLine {
    fn file(self) -> Option<usize> {
        match self {
            CommitLine::File(file) | CommitLine::Diff { file, .. } | CommitLine::NoDiff(file) => {
                Some(file)
            }
            _ => None,
        }
    }
}

/// The state of a commit buffer.
pub struct CommitView {
    pub root: PathBuf,
    pub commit: Commit,
    folded: HashSet<Part>,
    /// Files shown unfolded.
    open: HashSet<usize>,
    pub lines: Vec<CommitLine>,
}

/// Shows commit `id` of the working tree at `root` in a buffer of its own, `file`
/// unfolded if it is one of the commit's.
pub fn open(editor: &mut Editor, root: &Path, id: &str, file: Option<&Path>) -> CommandResult {
    let existing = editor
        .buffers()
        .iter()
        .find(|buffer| {
            buffer
                .local::<CommitView>()
                .is_some_and(|view| view.root == root && view.commit.id == id)
        })
        .map(|buffer| buffer.name().to_owned());
    if let Some(name) = existing {
        editor.switch_to_buffer(&name);
        return Ok(());
    }
    let commit = read(root, id)?;
    let focus = file.and_then(|file| commit.files.iter().position(|(path, _)| path == file));
    let name = format!("Commit {}", short(&commit.id));
    let mut buffer = Buffer::new(name, Text::default(), COMMIT_MODE)
        .with_directory(root.to_path_buf())
        .read_only();
    buffer.set_local(CommitView {
        root: root.to_path_buf(),
        commit,
        folded: HashSet::new(),
        open: focus.into_iter().collect(),
        lines: Vec::new(),
    });
    draw(&mut buffer);
    let first_file = buffer.local::<CommitView>().and_then(|view| {
        let target = focus.map_or(CommitLine::Heading(Part::Files), CommitLine::File);
        view.lines.iter().position(|line| *line == target)
    });
    editor.add_buffer(buffer);
    editor.set_point_at(first_file.unwrap_or_default(), 0);
    Ok(())
}

fn read(root: &Path, id: &str) -> Result<Commit, String> {
    let printed = output(
        git(root)
            .args(["-c", "core.quotePath=false", "show", FORMAT, DATE])
            .args(["--patch", "-M", "--diff-merges=first-parent"])
            .args(["--no-color", "--no-ext-diff", "--no-textconv", id, "--"]),
    )
    .map_err(|error| format!("git show: {error}"))?;
    let printed = String::from_utf8_lossy(&printed);
    let fields: Vec<&str> = printed.splitn(FIELDS + 1, '\0').collect();
    let &[id, parents, author, time, date, message, patch] = &fields[..] else {
        return Err("git show printed no commit".into());
    };
    let mut message = message.trim().lines();
    let subject = message.next().unwrap_or_default().to_owned();
    let body: Vec<String> = message.map(str::to_owned).collect();
    let body_start = body.iter().position(|line| !line.trim().is_empty());
    let body = body_start.map_or_else(Vec::new, |start| body[start..].to_vec());
    let parents: Vec<&str> = parents.split_whitespace().collect();
    Ok(Commit {
        id: id.to_owned(),
        parents: subjects(root, &parents),
        author: author.to_owned(),
        time: time.parse().unwrap_or_default(),
        date: date.to_owned(),
        subject,
        body,
        files: split_patch(patch),
    })
}

/// Each commit with its subject.
fn subjects(root: &Path, ids: &[&str]) -> Vec<(String, String)> {
    if ids.is_empty() {
        return Vec::new();
    }
    let printed = output(
        git(root)
            .args(["show", "-s", "--format=%H%x00%s"])
            .args(ids)
            .arg("--"),
    )
    .unwrap_or_default();
    let known: HashMap<String, String> = String::from_utf8_lossy(&printed)
        .lines()
        .filter_map(|line| {
            let (id, subject) = line.split_once('\0')?;
            Some((id.to_owned(), subject.to_owned()))
        })
        .collect();
    ids.iter()
        .map(|id| {
            (
                (*id).to_owned(),
                known.get(*id).cloned().unwrap_or_default(),
            )
        })
        .collect()
}

/// Rewrites the buffer's lines, point staying on what it was on.
fn draw(buffer: &mut Buffer) {
    let current_line = buffer.point_line();
    let Some(view) = buffer.local_mut::<CommitView>() else {
        return;
    };
    let current = view.lines.get(current_line).copied();
    let (lines, texts) = layout(view);
    let target = current
        .and_then(|current| {
            lines.iter().position(|line| *line == current).or_else(|| {
                let file = current.file()?;
                lines
                    .iter()
                    .position(|line| *line == CommitLine::File(file))
            })
        })
        .unwrap_or(current_line.min(lines.len().saturating_sub(1)));
    view.lines = lines;
    buffer.replace_text(Text::from(texts.join("\n").as_str()));
    let offset = buffer.text().line_start(target).unwrap_or_default();
    buffer.set_point(offset);
}

fn layout(view: &CommitView) -> (Vec<CommitLine>, Vec<String>) {
    let commit = &view.commit;
    let (mut lines, mut texts) = (Vec::new(), Vec::new());
    if !commit.body.is_empty() {
        lines.push(CommitLine::Heading(Part::Message));
        texts.push("Message".to_owned());
        if !view.folded.contains(&Part::Message) {
            for (index, text) in commit.body.iter().enumerate() {
                lines.push(CommitLine::Message(index));
                texts.push(text.clone());
            }
        }
    }
    lines.push(CommitLine::Heading(Part::Files));
    texts.push(format!("Files {}", commit.files.len()));
    if view.folded.contains(&Part::Files) {
        return (lines, texts);
    }
    for (index, (path, diff)) in commit.files.iter().enumerate() {
        lines.push(CommitLine::File(index));
        texts.push(format!("{} {}", diff.state().letter(), path.display()));
        if view.open.contains(&index) {
            push_diff(
                &mut lines,
                &mut texts,
                Some(diff),
                CommitLine::NoDiff(index),
                |hunk, line| CommitLine::Diff {
                    file: index,
                    hunk,
                    line,
                },
            );
        }
    }
    (lines, texts)
}

/// The window's commit buffer as an outline, if it is one.
pub fn outline(editor: &Editor, range: Range<usize>) -> Option<Outline> {
    let view = editor.window_buffer().local::<CommitView>()?;
    let lines = &view.lines;
    let rows = lines
        .get(range.start.min(lines.len())..range.end.min(lines.len()))
        .unwrap_or_default()
        .iter()
        .filter_map(|line| row(view, *line))
        .collect();
    Some(Outline {
        facts: facts(view),
        rows,
        toggle: TOGGLE,
        activate: VISIT,
        hints: HINTS,
        history: None,
        conflict: None,
        banner: None,
        toolbar: Vec::new(),
    })
}

fn row(view: &CommitView, line: CommitLine) -> Option<OutlineRow> {
    let commit = &view.commit;
    Some(match line {
        CommitLine::Heading(part) => OutlineRow::Heading {
            title: match part {
                Part::Message => "Message",
                Part::Files => "Files",
            }
            .into(),
            count: match part {
                Part::Message => commit.body.len(),
                Part::Files => commit.files.len(),
            },
            open: !view.folded.contains(&part),
            tone: Tone::Plain,
        },
        CommitLine::Message(index) => {
            OutlineRow::Diff(DiffRow::Context(commit.body.get(index)?.clone()))
        }
        CommitLine::File(index) => {
            let (path, diff) = commit.files.get(index)?;
            OutlineRow::File {
                state: diff.state(),
                name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                directory: directory_label(path),
                lines: diff.line_counts(),
                note: diff
                    .original()
                    .filter(|_| diff.hunks.is_empty())
                    .map(|original| (format!("from {}", original.display()), Tone::Faint)),
                open: view.open.contains(&index),
            }
        }
        CommitLine::Diff { file, hunk, line } => hunk_row(&commit.files.get(file)?.1, hunk, line)?,
        CommitLine::NoDiff(file) => {
            OutlineRow::Diff(DiffRow::Context(commit.files.get(file)?.1.summary()))
        }
    })
}

fn facts(view: &CommitView) -> Vec<Fact> {
    let commit = &view.commit;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    let mut facts = vec![
        Fact {
            label: "Commit",
            value: FactValue::Code(short(&commit.id)),
            note: commit.subject.clone(),
            note_is_title: true,
            buttons: Vec::new(),
        },
        Fact {
            label: "Author",
            value: FactValue::Code(commit.author.clone()),
            note: format!("{} · {}", commit.date, ago(now - commit.time)),
            note_is_title: false,
            buttons: Vec::new(),
        },
    ];
    for (index, (id, subject)) in commit.parents.iter().enumerate() {
        let (root, id_to_open) = (view.root.clone(), id.clone());
        facts.push(Fact {
            label: if index == 0 { "Parent" } else { "Merged" },
            value: FactValue::Code(short(id)),
            note: subject.clone(),
            note_is_title: false,
            buttons: vec![Button {
                label: "Open".into(),
                selected: false,
                action: Rc::new(move |editor| open(editor, &root, &id_to_open, None)),
            }],
        });
    }
    facts
}

fn short(id: &str) -> String {
    id.chars().take(SHORT_ID).collect()
}

/// Folds or unfolds the part at point, or shows or hides the diff of the file.
pub fn toggle(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    let point_line = buffer.point_line();
    let view = buffer
        .local_mut::<CommitView>()
        .ok_or(NOT_A_COMMIT_BUFFER)?;
    let line = *view.lines.get(point_line).ok_or("Nothing here")?;
    let back_to = match line {
        CommitLine::Heading(part) => {
            toggle_in(&mut view.folded, part);
            None
        }
        CommitLine::Message(_) => {
            view.folded.insert(Part::Message);
            Some(CommitLine::Heading(Part::Message))
        }
        CommitLine::File(file) => {
            toggle_in(&mut view.open, file);
            None
        }
        CommitLine::Diff { file, .. } | CommitLine::NoDiff(file) => {
            view.open.remove(&file);
            Some(CommitLine::File(file))
        }
    };
    let back_to = back_to.and_then(|target| view.lines.iter().position(|line| *line == target));
    if let Some(line) = back_to {
        buffer.set_point(buffer.text().line_start(line).unwrap_or_default());
    }
    draw(buffer);
    Ok(())
}

fn toggle_in<T: Eq + Hash>(set: &mut HashSet<T>, value: T) {
    if !set.remove(&value) {
        set.insert(value);
    }
}

/// Hides every file's diff when one is shown, else shows them all.
pub fn toggle_all(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    let view = buffer
        .local_mut::<CommitView>()
        .ok_or(NOT_A_COMMIT_BUFFER)?;
    if view.open.is_empty() {
        view.folded.remove(&Part::Files);
        view.open = (0..view.commit.files.len()).collect();
    } else {
        view.open.clear();
    }
    draw(buffer);
    Ok(())
}

/// The commit's file at point, and on a diff line, the line of the file it is at.
fn file_at_point(editor: &Editor) -> Result<(&CommitView, usize, Option<usize>), &'static str> {
    let buffer = editor.window_buffer();
    let view = buffer.local::<CommitView>().ok_or(NOT_A_COMMIT_BUFFER)?;
    let line = *view.lines.get(buffer.point_line()).ok_or("Nothing here")?;
    let file = line.file().ok_or("No file here")?;
    let target = match line {
        CommitLine::Diff { hunk, line, .. } => view.commit.files[file]
            .1
            .hunks
            .get(hunk)
            .map(|hunk| hunk.new_line(line.unwrap_or(0))),
        _ => None,
    };
    Ok((view, file, target))
}

/// Visits the file at point in the working tree, on a diff line at that line, like
/// Emacs `magit-diff-visit-worktree-file`.
pub fn visit(editor: &mut Editor) -> CommandResult {
    let is_heading = editor
        .window_buffer()
        .local::<CommitView>()
        .and_then(|view| view.lines.get(editor.window_buffer().point_line()))
        .is_some_and(|line| matches!(line, CommitLine::Heading(_)));
    if is_heading {
        return toggle(editor);
    }
    let (view, file, target) = file_at_point(editor)?;
    let path = view.root.join(&view.commit.files[file].0);
    if !path.exists() {
        return Err("Not in the working tree any more; o shows it as committed".into());
    }
    editor.visit_file(&path)?;
    if let Some(line) = target {
        editor.set_point_at(line.saturating_sub(1), 0);
    }
    Ok(())
}

/// Shows the file at point as the commit left it, or as it was before a commit that
/// deleted it, like Emacs `magit-diff-visit-file`.
pub fn open_file(editor: &mut Editor) -> CommandResult {
    let (view, file, target) = file_at_point(editor)?;
    let (path, diff) = &view.commit.files[file];
    if diff.binary {
        return Err("Binary file".into());
    }
    let id = short(&view.commit.id);
    let revision = if diff.state() == FileState::Deleted {
        format!("{}^", view.commit.id)
    } else {
        view.commit.id.clone()
    };
    let root = view.root.clone();
    let name = format!(
        "{} @ {id}",
        path.file_name().unwrap_or(path.as_os_str()).display()
    );
    let path = path.clone();
    if !editor.switch_to_buffer(&name) {
        let mut spec = revision.into_bytes();
        spec.push(b':');
        spec.extend_from_slice(path.as_os_str().as_encoded_bytes());
        let contents = output(
            git(&root)
                .arg("show")
                .arg(OsStr::from_bytes(&spec))
                .arg("--"),
        )
        .map_err(|error| format!("git show: {error}"))?;
        let mode = editor.mode_for(Some(&root.join(&path)));
        let directory = root.join(&path).parent().unwrap_or(&root).to_path_buf();
        let buffer = Buffer::new(
            name,
            Text::from(String::from_utf8_lossy(&contents).as_ref()),
            mode,
        )
        .with_directory(directory)
        .read_only();
        editor.add_buffer(buffer);
    }
    if let Some(line) = target {
        editor.set_point_at(line.saturating_sub(1), 0);
    }
    Ok(())
}
