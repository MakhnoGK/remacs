//! Resolving the conflicts of a rebase or merge stopped part way, then going on with it,
//! skipping the commit or giving up, like Emacs `smerge-mode` with `magit-rebase-continue`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use remacs_core::{
    Candidate, CommandResult, Completions, Editor, Keymap, ListItem, Message, Prompt,
};
use remacs_text::Text;

use crate::command::{git, output};
use crate::commands::{run, shown_status};
use crate::conflict::{self, Keep};
use crate::conflict_view::{conflicts_left, is_rebase};
use crate::monitor::Job;
use crate::operation::OperationKind;
use crate::repositories::Repositories;
use crate::status::Snapshot;
use crate::status_view::{
    Line, NOT_A_STATUS_BUFFER, Section, StatusView, directory_label, section_files,
};

const SUBJECT_LIMIT: usize = 50;
const ABORT: &str = "git-abort";
const SKIP: &str = "git-skip";
const CONTINUE_HINTS: &[(&str, &str)] = &[
    ("C-c C-c", "Continue"),
    ("M-s", "Skip commit"),
    ("C-c C-k", "Abort rebase"),
    ("C-g", "Back"),
];

pub fn keep_ours(editor: &mut Editor) -> CommandResult {
    keep_current(editor, Keep::Ours)
}

pub fn keep_theirs(editor: &mut Editor) -> CommandResult {
    keep_current(editor, Keep::Theirs)
}

pub fn keep_both(editor: &mut Editor) -> CommandResult {
    keep_current(editor, Keep::Both)
}

pub fn edit_current(editor: &mut Editor) -> CommandResult {
    let (path, index) = current_conflict(editor)?;
    edit_at(editor, &path, index)
}

pub fn next_conflict(editor: &mut Editor) -> CommandResult {
    step_conflict(editor, 1)
}

pub fn previous_conflict(editor: &mut Editor) -> CommandResult {
    step_conflict(editor, -1)
}

/// Whether point is on an unmerged file with conflict markers, where `o`, `t`, `b`, `e`,
/// `n` and `p` act on them.
pub fn in_conflict(editor: &Editor) -> bool {
    conflicted_at_point(editor).is_some()
}

fn conflicted_at_point(editor: &Editor) -> Option<(PathBuf, usize)> {
    let buffer = editor.window_buffer();
    let view = buffer.local::<StatusView>()?;
    let (Section::Unmerged, path) = view.line(buffer.point_line())?.file()? else {
        return None;
    };
    let count = editor
        .find_plugin_state::<Repositories>()?
        .get(&view.root)?
        .snapshot
        .as_ref()?
        .conflicts
        .get(path)?
        .count();
    (count > 0).then(|| (path.to_path_buf(), count))
}

fn current_conflict(editor: &Editor) -> Result<(PathBuf, usize), &'static str> {
    let (path, count) = conflicted_at_point(editor).ok_or("No conflict here")?;
    let view = editor
        .window_buffer()
        .local::<StatusView>()
        .ok_or(NOT_A_STATUS_BUFFER)?;
    let index = view.conflicts.current(&path, count);
    Ok((path, index))
}

fn keep_current(editor: &mut Editor, keep: Keep) -> CommandResult {
    let (path, index) = current_conflict(editor)?;
    keep_at(editor, &path, index, keep)
}

/// Resolves conflict `index` of `path` to the side `keep` says; once no conflict is left
/// the file is staged.
pub fn keep_at(editor: &mut Editor, path: &Path, index: usize, keep: Keep) -> CommandResult {
    let (root, snapshot) = shown_status(editor)?;
    let full = root.join(path);
    let text = read(editor, &full)?;
    let resolved =
        conflict::resolve(&text, index, keep).ok_or("The conflict changed; try again")?;
    write(editor, &full, &resolved.text)?;
    view_mut(editor)?.conflicts.record(
        path,
        resolved.start,
        resolved.replaced,
        resolved.kept,
        keep.said(is_rebase(&snapshot)),
    );
    if conflict::parse(&resolved.text).count() > 0 {
        return Ok(());
    }
    let path = path.to_path_buf();
    run(
        editor,
        &root,
        Box::new(move |root| {
            output(git(root).args(["add", "--"]).arg(&path))
                .map(drop)
                .map_err(|error| format!("git add: {error}"))
        }),
    )
}

/// Visits `path` at the start of conflict `index`.
pub fn edit_at(editor: &mut Editor, path: &Path, index: usize) -> CommandResult {
    let (root, snapshot) = shown_status(editor)?;
    let file = snapshot
        .conflicts
        .get(path)
        .ok_or("No conflicts in this file")?;
    let starts = file.starts();
    let line = file
        .regions
        .iter()
        .zip(starts)
        .filter(|(region, _)| matches!(region, conflict::Region::Conflict(_)))
        .nth(index)
        .map_or(0, |(_, start)| start);
    editor.visit_file(&root.join(path))?;
    editor.set_point_at(line, 0);
    Ok(())
}

/// Opens the next conflict of the file, past its last one the next unmerged file.
fn step_conflict(editor: &mut Editor, by: isize) -> CommandResult {
    let (path, count) = conflicted_at_point(editor).ok_or("No conflict here")?;
    let view = view_mut(editor)?;
    let current = view.conflicts.current(&path, count);
    if let Some(next) = current.checked_add_signed(by).filter(|next| *next < count) {
        view.conflicts.set_current(&path, next);
        return Ok(());
    }
    let files: Vec<(usize, PathBuf)> = view
        .lines
        .iter()
        .enumerate()
        .filter_map(|(line, each)| match each {
            Line::File(Section::Unmerged, file) => Some((line, file.clone())),
            _ => None,
        })
        .collect();
    let at = files
        .iter()
        .position(|(_, file)| *file == path)
        .unwrap_or_default();
    let target = (at as isize + by).rem_euclid(files.len() as isize) as usize;
    let (line, file) = files[target].clone();
    let first = if by > 0 { 0 } else { usize::MAX };
    view.conflicts.set_current(&file, first);
    editor.set_point_at(line, 0);
    Ok(())
}

/// Goes on with the rebase or merge once nothing is unmerged: a rebase after its message
/// is read, a merge by committing.
pub fn continue_operation(editor: &mut Editor) -> CommandResult {
    let (root, snapshot) = shown_status(editor)?;
    let operation = snapshot.operation.clone().ok_or("Nothing is in progress")?;
    let unmerged = snapshot
        .files
        .values()
        .filter(|entry| entry.conflicted)
        .count();
    if unmerged > 0 {
        return Err(match conflicts_left(&snapshot) {
            0 => "Stage the resolved files first (s)".into(),
            1 => "1 conflict left".into(),
            left => format!("{left} conflicts left").into(),
        });
    }
    let OperationKind::Rebase {
        step, total, next, ..
    } = &operation.kind
    else {
        return run(
            editor,
            &root,
            git_job(&["commit", "--no-edit"], "git commit"),
        );
    };
    let message_path = git_path(&root, "rebase-merge/message");
    // Git notes the conflicts in comments, which it leaves out of the commit.
    let message: String = message_path
        .as_ref()
        .and_then(|path| fs::read_to_string(path).ok())
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| format!("{line}\n"))
        .collect();
    let mut keymap = Keymap::default();
    keymap.bind("C-c C-k", ABORT);
    keymap.bind("M-s", SKIP);
    let next = next.clone();
    let prompt = Prompt {
        label: "Message".into(),
        initial_input: message.trim().to_owned(),
        wide: true,
        hints: CONTINUE_HINTS,
        keymap: Some(keymap),
        subject: format!(
            "Continue rebase · Step {step} of {total} · {} onto {}",
            operation.branch.as_deref().unwrap_or("HEAD"),
            operation.other.label()
        ),
        message: Some(Message {
            subject_limit: SUBJECT_LIMIT,
            placeholder: "More of the message (optional)",
            earlier: Vec::new(),
        }),
        items_title: "Resolved files",
        items_summary: snapshot.resolved.len().to_string(),
        items: resolved_items(editor, &snapshot),
        ..Prompt::default()
    };
    let complete = move |_: &Editor, _: &str| {
        let Some((action, commit)) = &next else {
            return Completions::default();
        };
        let tone = match action.as_str() {
            "squash" | "fixup" | "s" | "f" => remacs_core::Tone::Special,
            "drop" | "d" => remacs_core::Tone::Danger,
            _ => remacs_core::Tone::Plain,
        };
        let folds = matches!(action.as_str(), "squash" | "fixup" | "s" | "f");
        Completions::from(vec![Candidate {
            label: commit.id.clone(),
            annotation: if folds {
                format!("{} — folds into this commit", commit.subject)
            } else {
                commit.subject.clone()
            },
            group: Some("Next step".into()),
            action: Some((static_action(action), tone)),
            ..Candidate::new(commit.id.clone())
        }])
    };
    editor.read(prompt, complete, move |editor, message| {
        let job: Job = Box::new(move |root| {
            if let Some(path) = &message_path {
                fs::write(path, format!("{}\n", message.trim_end()))
                    .map_err(|error| format!("Writing the message: {error}"))?;
            }
            output(
                git(root)
                    .args(["rebase", "--continue"])
                    .env("GIT_EDITOR", "true"),
            )
            .map(drop)
            .map_err(|error| format!("git rebase: {error}"))
        });
        run(editor, &root, job)
    })
}

/// Leaves the commit being applied out and goes on.
pub fn skip(editor: &mut Editor) -> CommandResult {
    editor.cancel_minibuffer();
    let (root, snapshot) = shown_status(editor)?;
    if !is_rebase(&snapshot) {
        return Err("Only a rebase skips commits".into());
    }
    run(editor, &root, git_job(&["rebase", "--skip"], "git rebase"))
}

/// Gives the rebase or merge up, after asking, the branch back where it was.
pub fn abort(editor: &mut Editor) -> CommandResult {
    editor.cancel_minibuffer();
    let (root, snapshot) = shown_status(editor)?;
    let operation = snapshot.operation.clone().ok_or("Nothing is in progress")?;
    let branch = operation.branch.clone().unwrap_or_else(|| "HEAD".into());
    let (question, args) = match operation.kind {
        OperationKind::Rebase { .. } => (
            match &operation.original {
                Some(original) => format!("Abort the rebase? {branch} goes back to {original}"),
                None => "Abort the rebase?".into(),
            },
            ["rebase", "--abort"],
        ),
        OperationKind::Merge => (
            format!("Abort the merge? {branch} goes back as it was"),
            ["merge", "--abort"],
        ),
    };
    editor.ask_y_or_n(question, move |editor, yes| {
        if !yes {
            return Ok(());
        }
        run(editor, &root, git_job(&args, &format!("git {}", args[0])))
    })
}

pub const ABORT_COMMAND: &str = ABORT;
pub const SKIP_COMMAND: &str = SKIP;

fn git_job(args: &[&str], name: &str) -> Job {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let name = name.to_owned();
    Box::new(move |root| {
        output(git(root).args(&args).env("GIT_EDITOR", "true"))
            .map(drop)
            .map_err(|error| format!("{name}: {error}"))
    })
}

/// A path in the Git directory, e.g. `rebase-merge/message`.
fn git_path(root: &Path, name: &str) -> Option<PathBuf> {
    let printed =
        output(git(root).args(["rev-parse", "--path-format=absolute", "--git-path", name])).ok()?;
    let printed = String::from_utf8_lossy(&printed).trim().to_owned();
    (!printed.is_empty()).then(|| PathBuf::from(printed))
}

/// The files resolved in this step, with how their conflicts were resolved.
fn resolved_items(editor: &Editor, snapshot: &Snapshot) -> Vec<ListItem> {
    let view = editor.window_buffer().local::<StatusView>();
    section_files(snapshot, Section::Resolved)
        .into_iter()
        .map(|file| {
            let mut ways: BTreeMap<&str, usize> = BTreeMap::new();
            for resolution in view.map_or(&[][..], |view| view.conflicts.resolutions(&file.path)) {
                *ways.entry(resolution.said).or_default() += 1;
            }
            let ways: Vec<String> = ways
                .into_iter()
                .map(|(said, count)| format!("{count} {said}"))
                .collect();
            let mut detail = directory_label(&file.path);
            if !ways.is_empty() {
                detail = format!("{detail} · {}", ways.join(", "));
            }
            ListItem {
                icon: None,
                mark: None,
                label: file
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                detail,
                state: Some(file.state),
                lines: None,
            }
        })
        .collect()
}

/// The to-do list's word for a step, as one of ours.
fn static_action(action: &str) -> &'static str {
    match action {
        "p" | "pick" => "pick",
        "s" | "squash" => "squash",
        "f" | "fixup" => "fixup",
        "d" | "drop" => "drop",
        "r" | "reword" => "reword",
        "e" | "edit" => "edit",
        _ => "exec",
    }
}

/// The text of the file, from the buffer visiting it if one does.
fn read(editor: &Editor, path: &Path) -> Result<String, String> {
    match editor
        .buffers()
        .iter()
        .find(|buffer| buffer.path() == Some(path))
    {
        Some(buffer) if buffer.is_modified() => {
            Err(format!("Save {} first: it has changes", buffer.name()))
        }
        Some(buffer) => Ok(buffer.contents(0..buffer.text().len())),
        None => {
            fs::read_to_string(path).map_err(|error| format!("Reading {}: {error}", path.display()))
        }
    }
}

/// Writes the file, through the buffer visiting it if one does, so the two agree.
fn write(editor: &mut Editor, path: &Path, text: &str) -> Result<(), String> {
    let saved = match editor
        .buffers_mut()
        .iter_mut()
        .find(|buffer| buffer.path() == Some(path))
    {
        Some(buffer) => {
            buffer.replace_text(Text::from(text));
            buffer.save()
        }
        None => fs::write(path, text),
    };
    saved.map_err(|error| format!("Writing {}: {error}", path.display()))
}

fn view_mut(editor: &mut Editor) -> Result<&mut StatusView, &'static str> {
    editor
        .window_buffer_mut()
        .local_mut::<StatusView>()
        .ok_or(NOT_A_STATUS_BUFFER)
}
