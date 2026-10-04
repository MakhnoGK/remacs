//! Editing names in place, like Emacs wdired: the listing's lines become editable, and
//! applying renames the files whose lines changed, after updating references to them.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_core::{
    Buffer, Button, Changes, CommandResult, Completions, Editor, Elsewhere, FileEdits, Prompt,
    StatusItem, StatusSegment, Tone,
};
use remacs_files::abbreviate_home;

use crate::commands::NOT_A_LISTING;
use crate::entry::{Entry, PARENT, describe_entries, plural};
use crate::file_ops::rename_exclusive;
use crate::jobs::{self, Report, failure_message};
use crate::listing::{Listing, redraw};
use crate::references::{self, References, State};

pub const NOTICE_KEYS: &[(&str, &str)] = &[("C-c C-c", "Apply"), ("C-c C-k", "Cancel")];
pub const HINTS: &[(&str, &str)] = &[
    ("⇥", "Next name"),
    ("C-c C-c", "Apply"),
    ("C-c C-k", "Cancel"),
    ("M-%", "Replace in names"),
];
const NOT_EDITING: &str = "Not editing names";
const REPLACE_CATEGORY: &str = "wdired-replace";
const REPLACE_WITH_CATEGORY: &str = "wdired-replace-with";

/// The state of a listing whose names are being edited.
pub struct Editing {
    pub references: References,
    /// Whether applying also updates references to the renamed files.
    pub update_references: bool,
}

impl Default for Editing {
    fn default() -> Self {
        Self {
            references: References::default(),
            update_references: true,
        }
    }
}

/// A line whose name was edited.
pub struct Rename {
    pub line: usize,
    pub from: String,
    pub to: String,
    pub is_directory: bool,
}

pub fn is_editing(editor: &Editor) -> bool {
    editor
        .buffer()
        .local::<Listing>()
        .is_some_and(|listing| listing.editing.is_some())
}

/// Makes the names editable, like Emacs `wdired-change-to-wdired-mode`.
pub fn edit_names(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    let listing = buffer.local_mut::<Listing>().ok_or(NOT_A_LISTING)?;
    if !listing.is_loaded() {
        return Err("Still reading the directory".into());
    }
    let has_parent = has_parent(listing);
    listing.editing.get_or_insert_default();
    buffer.set_read_only(false);
    buffer.add_undo_boundary();
    if has_parent && buffer.point_line() == 0 {
        let first_name = buffer.text().line_start(1).unwrap_or_default();
        buffer.set_point(first_name);
    }
    Ok(())
}

/// Puts the names back as they are on disk, like `wdired-abort-changes`.
pub fn cancel(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    stop_editing(buffer)?;
    // What changed on disk meanwhile was not listed.
    if let Some(listing) = buffer.local::<Listing>() {
        listing.reader.reread();
    }
    editor.set_message("Edits discarded");
    Ok(())
}

/// Renames the files whose names were edited, after updating references to them, like
/// `wdired-finish-edit`.
pub fn apply(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    let editing = editing(buffer)?;
    let directory = buffer
        .default_directory()
        .ok_or(NOT_A_LISTING)?
        .to_path_buf();
    let renames = edited_names(buffer);
    check(buffer, &renames)?;
    let updates = match &editing.references.state {
        _ if !editing.update_references || renames.is_empty() => Vec::new(),
        State::Waiting => {
            return Err("Still finding references to update; try again in a moment".into());
        }
        State::Known { edits, .. } => edits.clone(),
        State::Unanswered => Vec::new(),
    };
    let updated = update_references(editor, updates)?;
    let buffer = editor.window_buffer_mut();
    let renamed = rename_entries(buffer, &renames);
    stop_editing(buffer)?;
    let what = match renames.as_slice() {
        [] => {
            editor.set_message("No changes to apply");
            return Ok(());
        }
        [single] => format!("{} to {}", single.from, single.to),
        _ => describe_entries(&renamed, ""),
    };
    let pairs: Vec<_> = renames
        .into_iter()
        .map(|rename| (rename.from, rename.to))
        .collect();
    jobs::run(editor, "Renaming…", move || {
        let (moved, failures) = rename_all(&directory, &pairs);
        let message =
            failure_message("rename", &failures).unwrap_or_else(|| match updated.as_slice() {
                [] => format!("Renamed {what}"),
                _ => format!("Renamed {what}; updated {}", updated.join(", ")),
            });
        Report {
            directories: vec![directory],
            moved,
            message,
        }
    });
    Ok(())
}

pub fn next_name(editor: &mut Editor) -> CommandResult {
    step_name(editor, true)
}

pub fn previous_name(editor: &mut Editor) -> CommandResult {
    step_name(editor, false)
}

/// Replaces text in every name; one undo takes it back.
pub fn replace_in_names(editor: &mut Editor) -> CommandResult {
    editing(editor.window_buffer())?;
    let prompt = Prompt {
        label: "Replace in names".into(),
        category: REPLACE_CATEGORY,
        placeholder: "text",
        hints: &[("↵", "Next"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        |editor, from| {
            if from.is_empty() {
                return Err("Nothing to replace".into());
            }
            let prompt = Prompt {
                label: format!("Replace “{from}” with"),
                category: REPLACE_WITH_CATEGORY,
                placeholder: "text",
                hints: &[("↵", "Replace"), ("C-g", "Cancel")],
                ..Prompt::default()
            };
            editor.read(
                prompt,
                |_, _| Completions::default(),
                move |editor, to| replace(editor, &from, &to),
            )
        },
    )
}

/// Switches whether applying updates references to the renamed files.
pub fn toggle_reference_updates(editor: &mut Editor) -> CommandResult {
    let editing = editing_mut(editor.window_buffer_mut())?;
    editing.update_references = !editing.update_references;
    Ok(())
}

/// Keeps one name per line and `..` as it is by undoing a command that changed that, then
/// asks about the references the renames would update. Runs after every command.
pub fn after_command(editor: &mut Editor) {
    if editing(editor.window_buffer()).is_err() {
        return;
    }
    keep_lines(editor);
    ask_about_references(editor);
}

pub fn status(editor: &Editor) -> Option<StatusSegment> {
    let buffer = editor.window_buffer();
    editing(buffer).ok()?;
    let text = match edited_names(buffer).len() {
        0 => "Editing".to_owned(),
        count => format!("Editing · {count} {}", plural(count, "change")),
    };
    Some(StatusSegment {
        items: vec![StatusItem::toned(text, Tone::Warning)],
        command: None,
        active: false,
    })
}

/// The renames so far and what they would update elsewhere.
pub fn changes(buffer: &Buffer, editing: &Editing) -> Changes {
    let renames = edited_names(buffer);
    let summary = match renames.len() {
        0 => "None yet".to_owned(),
        count => format!("{count} {}", plural(count, "rename")),
    };
    let shown = |name: &str, is_directory: bool| {
        if is_directory {
            format!("{name}/")
        } else {
            name.to_owned()
        }
    };
    let edits = renames
        .iter()
        .map(|rename| {
            (
                shown(&rename.from, rename.is_directory),
                shown(&rename.to, rename.is_directory),
            )
        })
        .collect();
    let state = &editing.references.state;
    let has_updates = !renames.is_empty()
        && match state {
            State::Unanswered => false,
            State::Waiting => true,
            State::Known { diffs, .. } => !diffs.is_empty(),
        };
    let elsewhere = match state {
        _ if !has_updates || !editing.update_references => Elsewhere::Nothing,
        State::Known { diffs, .. } => Elsewhere::Files(diffs.clone()),
        _ => Elsewhere::Checking,
    };
    let options = has_updates
        .then(|| Button {
            label: "Update references".into(),
            selected: editing.update_references,
            action: Rc::new(toggle_reference_updates),
        })
        .into_iter()
        .collect();
    Changes {
        summary,
        edits,
        elsewhere,
        options,
    }
}

/// Lines whose names were edited, in order.
pub fn edited_names(buffer: &Buffer) -> Vec<Rename> {
    let Some(listing) = buffer.local::<Listing>() else {
        return Vec::new();
    };
    let text = buffer.contents(0..buffer.text().len());
    text.split('\n')
        .zip(&listing.entries)
        .enumerate()
        .filter(|(_, (line, entry))| {
            *line != entry.name && entry.name != PARENT && !entry.name.contains('\n')
        })
        .map(|(line, (to, entry))| Rename {
            line,
            from: entry.name.clone(),
            to: to.to_owned(),
            is_directory: entry.is_directory,
        })
        .collect()
}

fn editing(buffer: &Buffer) -> Result<&Editing, &'static str> {
    buffer
        .local::<Listing>()
        .ok_or(NOT_A_LISTING)?
        .editing
        .as_ref()
        .ok_or(NOT_EDITING)
}

fn editing_mut(buffer: &mut Buffer) -> Result<&mut Editing, &'static str> {
    buffer
        .local_mut::<Listing>()
        .ok_or(NOT_A_LISTING)?
        .editing
        .as_mut()
        .ok_or(NOT_EDITING)
}

fn has_parent(listing: &Listing) -> bool {
    listing
        .entries
        .first()
        .is_some_and(|entry| entry.name == PARENT)
}

/// Makes the buffer read-only again, its lines the entries' names.
fn stop_editing(buffer: &mut Buffer) -> Result<Editing, &'static str> {
    let listing = buffer.local_mut::<Listing>().ok_or(NOT_A_LISTING)?;
    let editing = listing.editing.take().ok_or(NOT_EDITING)?;
    buffer.set_read_only(true);
    redraw(buffer);
    Ok(editing)
}

/// Why `name` cannot be a name in the listed directory, if it cannot; moving elsewhere
/// is `R`'s job.
fn invalid_name(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        Some("is empty")
    } else if name.contains('/') {
        Some("contains /")
    } else if name == "." || name == PARENT {
        Some("is reserved")
    } else if name.contains('\0') {
        Some("contains a NUL")
    } else {
        None
    }
}

/// Fails on a name that cannot be, or that two entries would share as a case-insensitive
/// volume sees them.
fn check(buffer: &Buffer, renames: &[Rename]) -> Result<(), String> {
    for rename in renames {
        if let Some(reason) = invalid_name(&rename.to) {
            return Err(format!(
                "Cannot rename {}: the new name {reason}",
                rename.from
            ));
        }
    }
    let text = buffer.contents(0..buffer.text().len());
    let mut names: HashMap<String, usize> = HashMap::new();
    for name in text.split('\n') {
        *names.entry(name.to_lowercase()).or_default() += 1;
    }
    match renames
        .iter()
        .find(|rename| names[&rename.to.to_lowercase()] > 1)
    {
        Some(rename) => Err(format!("Two files would be named {}", rename.to)),
        None => Ok(()),
    }
}

/// Makes the edits, saving the files that had no unsaved changes so that they match the
/// renamed files on disk; returns the buffers edited.
fn update_references(editor: &mut Editor, updates: Vec<FileEdits>) -> Result<Vec<String>, String> {
    let mut updated = Vec::new();
    for edits in updates {
        let path = edits.path.clone();
        let failed =
            |error: io::Error| format!("Could not update {}: {error}", abbreviate_home(&path));
        let unsaved = editor
            .buffers()
            .iter()
            .any(|buffer| buffer.path() == Some(path.as_path()) && buffer.is_modified());
        let buffer = editor.edit_file(edits).map_err(failed)?;
        if !unsaved {
            buffer.save().map_err(failed)?;
        }
        updated.push(buffer.name().to_owned());
    }
    Ok(updated)
}

/// Lists the new names at once, marks and all; the read after renaming corrects any
/// that failed. Returns the entries as they were.
fn rename_entries(buffer: &mut Buffer, renames: &[Rename]) -> Vec<Entry> {
    let Some(listing) = buffer.local_mut::<Listing>() else {
        return Vec::new();
    };
    let marks: Vec<_> = renames
        .iter()
        .map(|rename| listing.marks.remove(&rename.from))
        .collect();
    let mut renamed = Vec::new();
    for (rename, mark) in renames.iter().zip(marks) {
        let entry = &mut listing.entries[rename.line];
        renamed.push(entry.clone());
        entry.name.clone_from(&rename.to);
        if let Some(mark) = mark {
            listing.marks.insert(rename.to.clone(), mark);
        }
    }
    renamed
}

/// Renames in two steps through temporary names, so that names being swapped, passed on
/// or only changing case never meet; nothing already there is replaced.
fn rename_all(
    directory: &Path,
    renames: &[(String, String)],
) -> (Vec<(PathBuf, PathBuf)>, Vec<String>) {
    let mut failures = Vec::new();
    let mut staged = Vec::new();
    for (index, (from, to)) in renames.iter().enumerate() {
        let temporary = directory.join(format!(".remacs-rename-{}-{index}", std::process::id()));
        match rename_exclusive(&directory.join(from), &temporary) {
            Ok(()) => staged.push((temporary, from, to)),
            Err(error) => failures.push(format!("{from}: {error}")),
        }
    }
    let mut moved = Vec::new();
    for (temporary, from, to) in staged {
        let (source, target) = (directory.join(from), directory.join(to));
        match rename_exclusive(&temporary, &target) {
            Ok(()) => moved.push((source, target)),
            Err(error) => {
                failures.push(format!("{from}: {error}"));
                let _ = rename_exclusive(&temporary, &source);
            }
        }
    }
    (moved, failures)
}

/// Moves to the end of the next or previous name, around from the last to the first.
fn step_name(editor: &mut Editor, forward: bool) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    editing(buffer)?;
    let first = usize::from(buffer.local::<Listing>().is_some_and(has_parent));
    let count = buffer.text().line_count();
    if count <= first {
        return Ok(());
    }
    let line = buffer.point_line();
    let target = match forward {
        _ if line < first => first,
        true if line + 1 < count => line + 1,
        true => first,
        false if line > first => line - 1,
        false => count - 1,
    };
    let end = buffer
        .text()
        .line_range(target)
        .map_or(0, |range| range.end);
    buffer.set_point(end);
    Ok(())
}

fn replace(editor: &mut Editor, from: &str, to: &str) -> CommandResult {
    if from.contains('\n') || to.contains('\n') {
        return Err("Names stay on one line".into());
    }
    let buffer = editor.window_buffer_mut();
    editing(buffer)?;
    let first = usize::from(buffer.local::<Listing>().is_some_and(has_parent));
    let mut edits = Vec::new();
    for line in first..buffer.text().line_count() {
        let Some(range) = buffer.text().line_range(line) else {
            continue;
        };
        let name = buffer.contents(range.clone());
        edits.extend(name.match_indices(from).map(|(at, _)| {
            let start = range.start + at;
            (start..start + from.len(), to.to_owned())
        }));
    }
    if edits.is_empty() {
        return Err(format!("No name contains “{from}”").into());
    }
    let count = edits.len();
    buffer.add_undo_boundary();
    buffer.apply_edits(edits);
    editor.set_message(format!("Replaced {count} {}", plural(count, "occurrence")));
    Ok(())
}

fn keep_lines(editor: &mut Editor) {
    let buffer = editor.window_buffer_mut();
    let Some(problem) = line_problem(buffer) else {
        return;
    };
    if !buffer.undo() || line_problem(buffer).is_some() {
        // Nothing left to undo it with: back to the names on disk, still editing.
        let editing = buffer
            .local_mut::<Listing>()
            .and_then(|listing| listing.editing.take());
        redraw(buffer);
        if let Some(listing) = buffer.local_mut::<Listing>() {
            listing.editing = editing;
        }
    }
    editor.set_message(problem);
}

fn line_problem(buffer: &Buffer) -> Option<&'static str> {
    let listing = buffer.local::<Listing>()?;
    let text = buffer.text();
    if text.line_count() != listing.entries.len() {
        return Some("Names stay one per line");
    }
    let parent_changed = has_parent(listing)
        && text
            .line_range(0)
            .is_some_and(|range| buffer.contents(range) != PARENT);
    parent_changed.then_some(".. cannot be renamed")
}

fn ask_about_references(editor: &mut Editor) {
    let buffer = editor.window_buffer();
    let (Some(directory), Ok(editing)) = (buffer.default_directory(), editing(buffer)) else {
        return;
    };
    let renames: Vec<_> = edited_names(buffer)
        .into_iter()
        .filter(|rename| invalid_name(&rename.to).is_none())
        .map(|rename| (directory.join(rename.from), directory.join(rename.to)))
        .collect();
    if renames == editing.references.renames {
        return;
    }
    let references = references::ask(editor, renames);
    if let Ok(editing) = editing_mut(editor.window_buffer_mut()) {
        editing.references = references;
    }
}
