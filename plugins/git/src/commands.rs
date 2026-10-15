use std::ffi::OsString;
use std::path::{Path, PathBuf};

use remacs_core::{Buffer, CommandResult, Dialog, Editor, Icon, ListItem};
use remacs_text::Text;
use trash::TrashContext;
use trash::macos::{DeleteMethod, TrashContextExtMacos};

use crate::command::{git, output, output_with_input};
use crate::monitor::Job;
use crate::repositories::{Repositories, working_tree};
use crate::stash;
use crate::status::Snapshot;
use crate::status_view::{GIT_MODE, Line, NOT_A_STATUS_BUFFER, Section, StatusView, section_files};

const BUFFER_NAME: &str = "Git";

/// Shows the status of the working tree the window's directory is in, like Emacs
/// `magit-status`.
pub fn status(editor: &mut Editor) -> CommandResult {
    let directory = editor
        .window_buffer()
        .default_directory()
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .ok_or("No directory")?;
    let root = working_tree(&directory).ok_or("Not in a Git working tree")?;
    let existing = editor
        .buffers()
        .iter()
        .find(|buffer| {
            buffer
                .local::<StatusView>()
                .is_some_and(|view| view.root == root)
        })
        .map(|buffer| buffer.name().to_owned());
    if let Some(name) = existing {
        editor.switch_to_buffer(&name);
        return Ok(());
    }
    let mut buffer = Buffer::new(BUFFER_NAME, Text::default(), GIT_MODE)
        .with_directory(root.clone())
        .read_only();
    buffer.set_local(StatusView::new(root));
    editor.add_buffer(buffer);
    Ok(())
}

/// Folds or unfolds the section at point, or shows or hides the diff of the file.
pub fn toggle(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    let point_line = buffer.point_line();
    let view = buffer
        .local_mut::<StatusView>()
        .ok_or(NOT_A_STATUS_BUFFER)?;
    let line = view.line(point_line).cloned().ok_or("Nothing here")?;
    match &line {
        Line::Heading(section) => view.toggle_section(*section),
        Line::File(section, path) if section.has_diffs() => view.toggle_file(*section, path),
        Line::File(..) => return Err("No diff to show".into()),
        Line::Diff { .. } | Line::NoDiff(..) | Line::MoreFiles(_) => {
            let (section, path) = line.file().ok_or("Nothing here")?;
            let file = Line::File(section, path.to_path_buf());
            let file_line = view.lines.iter().position(|each| *each == file);
            view.toggle_file(section, path);
            if let Some(file_line) = file_line {
                editor.set_point_at(file_line, 0);
            }
        }
        Line::Stash(index) => return toggle_stash(editor, *index),
        Line::StashFile { stash, .. } | Line::StashDiff { stash, .. } => {
            view.fold_stash(stash);
            let stash_line = view.lines[..point_line]
                .iter()
                .rposition(|each| matches!(each, Line::Stash(_)));
            if let Some(stash_line) = stash_line {
                editor.set_point_at(stash_line, 0);
            }
        }
    }
    Ok(())
}

/// Hides every file's diff when one is shown, else shows them all, like Emacs
/// `magit-section-cycle-global`.
pub fn toggle_all(editor: &mut Editor) -> CommandResult {
    let any_open = status_view(editor)?.any_file_open();
    if any_open {
        collapse_all(editor)
    } else {
        expand_all(editor)
    }
}

pub fn expand_all(editor: &mut Editor) -> CommandResult {
    let (_, snapshot) = shown_status(editor)?;
    status_view(editor)?.open_all(&snapshot);
    Ok(())
}

pub fn collapse_all(editor: &mut Editor) -> CommandResult {
    status_view(editor)?.close_all();
    Ok(())
}

fn status_view(editor: &mut Editor) -> Result<&mut StatusView, &'static str> {
    editor
        .window_buffer_mut()
        .local_mut::<StatusView>()
        .ok_or(NOT_A_STATUS_BUFFER)
}

/// Unfolds the stash to show its files and their diffs, or folds it.
fn toggle_stash(editor: &mut Editor, index: usize) -> CommandResult {
    let (root, snapshot) = shown_status(editor)?;
    let id = snapshot
        .details
        .as_ref()
        .and_then(|details| details.stashes.get(index))
        .map(|stash| stash.id.clone())
        .ok_or("The stash is gone")?;
    let view = editor
        .window_buffer_mut()
        .local_mut::<StatusView>()
        .ok_or(NOT_A_STATUS_BUFFER)?;
    if !view.fold_stash(&id) {
        view.show_stash(id.clone(), stash::contents(&root, &id)?);
    }
    Ok(())
}

/// Stages the section, file or hunk at point, like Emacs `magit-stage`.
pub fn stage(editor: &mut Editor) -> CommandResult {
    let (root, snapshot, line) = at_point(editor)?;
    let job = match &line {
        Line::Heading(Section::Unstaged) => git_job(["add", "--update", "--", "."]),
        Line::Heading(section @ (Section::Untracked | Section::Unmerged)) => {
            let paths = section_files(&snapshot, *section)
                .into_iter()
                .map(|file| file.path);
            git_job(args(["add", "--"], paths))
        }
        Line::File(Section::Unstaged | Section::Untracked | Section::Unmerged, path)
        | Line::Diff {
            section: Section::Untracked,
            path,
            ..
        }
        | Line::NoDiff(Section::Untracked, path)
        | Line::MoreFiles(path) => git_job(args(["add", "--all", "--"], [path.clone()])),
        Line::Diff {
            section: Section::Unstaged,
            path,
            hunk,
            ..
        } => apply_job(&snapshot, path, *hunk, false, &["--cached"])?,
        Line::Stash(_) | Line::Heading(Section::Stashes) => return Err("Not a change".into()),
        _ => return Err("Already staged".into()),
    };
    run(editor, &root, job)
}

/// Unstages the section, file or hunk at point, like Emacs `magit-unstage`.
pub fn unstage(editor: &mut Editor) -> CommandResult {
    let (root, snapshot, line) = at_point(editor)?;
    let unborn = snapshot.is_unborn();
    let job = match &line {
        Line::Heading(Section::Staged) if unborn => {
            git_job(["rm", "-r", "-q", "--cached", "--", "."])
        }
        Line::Heading(Section::Staged) => git_job(["reset", "-q", "--"]),
        Line::Heading(Section::Resolved) => git_job(args(
            ["reset", "-q", "--"],
            section_files(&snapshot, Section::Resolved)
                .into_iter()
                .map(|file| file.path),
        )),
        Line::File(section @ (Section::Staged | Section::Resolved), path) => {
            let original = section_files(&snapshot, *section)
                .into_iter()
                .find(|file| file.path == *path)
                .and_then(|file| file.original);
            let paths = [path.clone()].into_iter().chain(original);
            if unborn {
                git_job(args(["rm", "-r", "-q", "--cached", "--"], paths))
            } else {
                git_job(args(["reset", "-q", "--"], paths))
            }
        }
        Line::Diff {
            section: Section::Staged | Section::Resolved,
            path,
            hunk,
            ..
        } => apply_job(&snapshot, path, *hunk, true, &["--cached", "--reverse"])?,
        _ => return Err("Not staged".into()),
    };
    run(editor, &root, job)
}

/// Throws away the unstaged changes at point, or moves untracked files to the Trash,
/// after asking, like Emacs `magit-discard`.
pub fn discard(editor: &mut Editor) -> CommandResult {
    let (root, snapshot, line) = at_point(editor)?;
    let untracked = |paths: Vec<PathBuf>| {
        let root = root.clone();
        let job: Job = Box::new(move |_| trash_all(&root, &paths));
        (paths_dialog_items(&snapshot, Section::Untracked), job)
    };
    let (title, explanation, items, job) = match &line {
        Line::Heading(Section::Unstaged) => (
            "Discard all unstaged changes?".to_owned(),
            "The files go back to what is staged, or to the last commit.",
            paths_dialog_items(&snapshot, Section::Unstaged),
            git_job(["restore", "--worktree", "--", "."]),
        ),
        Line::File(Section::Unstaged, path) => (
            format!("Discard the changes to {}?", file_name(path)),
            "The file goes back to what is staged, or to the last commit.",
            Vec::new(),
            git_job(args(["restore", "--worktree", "--"], [path.clone()])),
        ),
        Line::Diff {
            section: Section::Unstaged,
            path,
            hunk,
            ..
        } => (
            format!("Discard this change to {}?", file_name(path)),
            "The lines go back to what is staged, or to the last commit.",
            Vec::new(),
            apply_job(&snapshot, path, *hunk, false, &["--reverse"])?,
        ),
        Line::Heading(Section::Untracked) => {
            let paths = section_files(&snapshot, Section::Untracked)
                .into_iter()
                .map(|file| file.path)
                .collect();
            let (items, job) = untracked(paths);
            (
                "Move the untracked files to the Trash?".to_owned(),
                "You can still restore them from the Trash.",
                items,
                job,
            )
        }
        Line::File(Section::Untracked, path)
        | Line::Diff {
            section: Section::Untracked,
            path,
            ..
        }
        | Line::NoDiff(Section::Untracked, path)
        | Line::MoreFiles(path) => {
            let (_, job) = untracked(vec![path.clone()]);
            (
                format!("Move {} to the Trash?", file_name(path)),
                "You can still restore it from the Trash.",
                Vec::new(),
                job,
            )
        }
        Line::Heading(Section::Staged | Section::Resolved)
        | Line::File(Section::Staged | Section::Resolved, _)
        | Line::Diff { .. } => {
            return Err("Unstage it first (u), then discard".into());
        }
        Line::Stash(_) => return stash::drop_stash(editor),
        _ => return Err("Nothing to discard".into()),
    };
    editor.ask_dialog(
        false,
        move |_| Dialog {
            title: title.clone(),
            explanation: explanation.into(),
            items: items.clone(),
            option: None,
            yes_label: "Discard".into(),
            no_label: "Cancel",
            destructive: true,
        },
        move |editor, yes, _| {
            if yes {
                run(editor, &root, job)?;
            }
            Ok(())
        },
    )
}

/// Visits the file at point; on a diff line, at that line.
pub fn visit(editor: &mut Editor) -> CommandResult {
    let (root, snapshot, line) = at_point(editor)?;
    let (section, path) = match &line {
        Line::Heading(_) | Line::Stash(_) => return toggle(editor),
        _ => line.file().ok_or("No file here")?,
    };
    let target = match &line {
        Line::Diff {
            hunk,
            line: diff_line,
            ..
        } => {
            let staged = section.is_staged();
            let key = crate::status::DiffKey {
                staged,
                path: path.to_path_buf(),
            };
            snapshot
                .diffs
                .get(&key)
                .and_then(|diff| diff.hunks.get(*hunk))
                .map(|hunk| hunk.new_line(diff_line.unwrap_or(0)))
        }
        _ => None,
    };
    editor.visit_file(&root.join(path))?;
    if let Some(line) = target {
        editor.set_point_at(line.saturating_sub(1), 0);
    }
    Ok(())
}

/// Reads the status again, though changes show up by themselves.
pub fn refresh(editor: &mut Editor) -> CommandResult {
    let (root, ..) = view_root(editor)?;
    repository_monitor(editor, &root, |monitor| monitor.refresh())
}

/// Shows the buffer shown before this one, like Emacs `magit-mode-bury-buffer`.
pub fn quit(editor: &mut Editor) -> CommandResult {
    let other = editor
        .buffers()
        .get(1)
        .map(|buffer| buffer.name().to_owned())
        .ok_or("No other buffer")?;
    editor.switch_to_buffer(&other);
    Ok(())
}

fn view_root(editor: &Editor) -> Result<(PathBuf, usize), &'static str> {
    let buffer = editor.window_buffer();
    let view = buffer.local::<StatusView>().ok_or(NOT_A_STATUS_BUFFER)?;
    Ok((view.root.clone(), buffer.point_line()))
}

/// The working tree, its status as shown and what the line at point stands for.
fn at_point(editor: &Editor) -> Result<(PathBuf, Snapshot, Line), &'static str> {
    let (root, snapshot) = shown_status(editor)?;
    let point_line = editor.window_buffer().point_line();
    let view = editor
        .window_buffer()
        .local::<StatusView>()
        .ok_or(NOT_A_STATUS_BUFFER)?;
    let line = view.line(point_line).cloned().ok_or("Nothing here")?;
    Ok((root, snapshot, line))
}

/// The working tree of the status buffer and its status as shown.
pub fn shown_status(editor: &Editor) -> Result<(PathBuf, Snapshot), &'static str> {
    let (root, _) = view_root(editor)?;
    let snapshot = editor
        .find_plugin_state::<Repositories>()
        .and_then(|repositories| repositories.get(&root)?.snapshot.clone())
        .ok_or("Still reading the status")?;
    Ok((root, snapshot))
}

/// Runs `job`, saying `doing` now and `done` once it succeeded, for one that takes long,
/// like a push.
pub fn run_saying(
    editor: &mut Editor,
    root: &Path,
    job: Job,
    doing: String,
    done: String,
) -> CommandResult {
    repository_monitor(editor, root, |monitor| monitor.run_saying(job, done))?;
    editor.set_message(doing);
    Ok(())
}

pub fn run(editor: &mut Editor, root: &Path, job: Job) -> CommandResult {
    repository_monitor(editor, root, |monitor| monitor.run(job))
}

fn repository_monitor(
    editor: &Editor,
    root: &Path,
    action: impl FnOnce(&crate::monitor::Monitor),
) -> CommandResult {
    let repository = editor
        .find_plugin_state::<Repositories>()
        .and_then(|repositories| repositories.get(root))
        .ok_or("The working tree is not being watched")?;
    action(&repository.monitor);
    Ok(())
}

fn args<const N: usize>(
    fixed: [&str; N],
    paths: impl IntoIterator<Item = PathBuf>,
) -> Vec<OsString> {
    fixed
        .into_iter()
        .map(OsString::from)
        .chain(paths.into_iter().map(PathBuf::into_os_string))
        .collect()
}

pub fn git_job(args: impl IntoIterator<Item = impl Into<OsString>>) -> Job {
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    Box::new(move |root| {
        output(git(root).args(&args))
            .map(drop)
            .map_err(|error| format!("git {}: {error}", args[0].to_string_lossy()))
    })
}

/// Applies hunk `hunk` of `path`'s staged or unstaged diff with `flags`.
fn apply_job(
    snapshot: &Snapshot,
    path: &Path,
    hunk: usize,
    staged: bool,
    flags: &[&str],
) -> Result<Job, &'static str> {
    let key = crate::status::DiffKey {
        staged,
        path: path.to_path_buf(),
    };
    let patch = snapshot
        .diffs
        .get(&key)
        .and_then(|diff| diff.patch(hunk))
        .ok_or("The diff changed; try again")?;
    let flags: Vec<String> = flags.iter().map(|flag| (*flag).to_owned()).collect();
    Ok(Box::new(move |root| {
        output_with_input(
            git(root)
                .arg("apply")
                .args(&flags)
                .args(["--whitespace=nowarn", "-"]),
            patch.as_bytes(),
        )
        .map(drop)
        .map_err(|error| format!("git apply: {error}"))
    }))
}

fn trash_all(root: &Path, paths: &[PathBuf]) -> Result<(), String> {
    let mut trash = TrashContext::default();
    // Finder's way asks for permission to script it, and plays a sound.
    trash.set_delete_method(DeleteMethod::NsFileManager);
    let full: Vec<PathBuf> = paths.iter().map(|path| root.join(path)).collect();
    trash
        .delete_all(&full)
        .map_err(|error| format!("Could not move to the Trash: {error}"))
}

fn paths_dialog_items(snapshot: &Snapshot, section: Section) -> Vec<ListItem> {
    section_files(snapshot, section)
        .into_iter()
        .map(|file| ListItem {
            icon: Some(Icon::File),
            mark: None,
            label: file.path.display().to_string(),
            detail: String::new(),
            state: None,
            lines: None,
        })
        .collect()
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
