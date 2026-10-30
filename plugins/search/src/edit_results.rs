//! Editing the lines a project search found in a buffer of their own, then writing the
//! changed ones back to their files, like Emacs wgrep.

use std::collections::BTreeMap;
use std::path::PathBuf;

use remacs_core::{Buffer, CommandResult, Editor, FUNDAMENTAL_MODE, FileEdits};
use remacs_text::{Snapshot, Text};

use crate::project;
use crate::project_replace::plural;

pub const EDIT_SEARCH_RESULTS: &str = "edit-search-results";
pub const FINISH: &str = "edit-search-results-finish";
pub const ABORT: &str = "edit-search-results-abort";

const BUFFER_NAME: &str = "*search results*";
const NOT_RESULTS: &str = "Not in search results";
const LAYOUT_CHANGED: &str =
    "Lines were added, removed or their file:line changed; edit only the text after it";

/// The lines of a results buffer as listed: `file:line:` and the line's text after it.
struct Listed(Vec<ListedLine>);

struct ListedLine {
    path: PathBuf,
    line: usize,
    prefix: String,
    text: String,
}

pub fn is_editing(editor: &Editor) -> bool {
    editor.buffer().local::<Listed>().is_some()
}

/// Closes the project search and lists its lines, `file:line:text`, in a buffer whose
/// texts can be edited, like Emacs `embark-export` and `wgrep-change-to-wgrep-mode`.
pub fn edit_search_results(editor: &mut Editor) -> CommandResult {
    let (root, found) = project::close(editor)?;
    let mut listed = Vec::new();
    for (file, lines) in found {
        let path = root.join(&file);
        // As searched: an open buffer's text, unsaved changes and all.
        let snapshot = match editor.buffers().iter().find(|buffer| buffer.path() == Some(&path)) {
            Some(buffer) => buffer.text().snapshot(),
            None => Text::open(&path)?.snapshot(),
        };
        for line in lines {
            listed.push(ListedLine {
                path: path.clone(),
                line,
                prefix: format!("{file}:{}:", line + 1),
                text: line_text(&snapshot, line).unwrap_or_default(),
            });
        }
    }
    if listed.is_empty() {
        return Err("No lines to edit".into());
    }
    let contents: Vec<_> =
        listed.iter().map(|line| format!("{}{}", line.prefix, line.text)).collect();
    let mut buffer =
        Buffer::new(BUFFER_NAME, Text::from(contents.join("\n").as_str()), FUNDAMENTAL_MODE)
            .with_directory(root);
    buffer.set_local(Listed(listed));
    editor.add_buffer(buffer);
    editor.set_message("Edit the lines; C-c C-c writes them, C-c C-k discards them");
    Ok(())
}

/// Writes the edited lines to their files' buffers, saving those that had no unsaved
/// changes, and to the files no buffer visits; a line whose file changed since the search
/// is left alone. Like `wgrep-finish-edit`.
pub fn finish(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    let Listed(listed) = buffer.local::<Listed>().ok_or(NOT_RESULTS)?;
    let text = buffer.text();
    if text.line_count() != listed.len() {
        return Err(LAYOUT_CHANGED.into());
    }
    let mut changes: BTreeMap<PathBuf, Vec<(usize, String, String)>> = BTreeMap::new();
    for (index, listed_line) in listed.iter().enumerate() {
        let now = line_text(text, index).unwrap_or_default();
        let edited = now.strip_prefix(&listed_line.prefix).ok_or(LAYOUT_CHANGED)?;
        if edited != listed_line.text {
            changes.entry(listed_line.path.clone()).or_default().push((
                listed_line.line,
                listed_line.text.clone(),
                edited.to_owned(),
            ));
        }
    }
    let (mut written, mut skipped, mut errors) = (0, 0, Vec::new());
    for (path, lines) in changes {
        match write_lines(editor, path.clone(), lines) {
            Ok((wrote, stale)) => (written, skipped) = (written + wrote, skipped + stale),
            Err(error) => errors.push(format!("{}: {error}", path.display())),
        }
    }
    let name = editor.window_buffer().name().to_owned();
    if let Some(error) = errors.first() {
        return Err(format!("Could not write {error}").into());
    }
    editor.kill_buffer(&name);
    let message = format!("Wrote {}", plural(written, "line"));
    match skipped {
        0 => editor.set_done(message),
        _ => editor.set_error(format!(
            "{message}; left {} that changed since the search",
            plural(skipped, "line")
        )),
    }
    Ok(())
}

/// Discards the edits, like `wgrep-abort-changes`.
pub fn abort(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    buffer.local::<Listed>().ok_or(NOT_RESULTS)?;
    let name = buffer.name().to_owned();
    editor.kill_buffer(&name);
    editor.set_message("Edits discarded");
    Ok(())
}

/// Replaces `lines`, each its number, text when found and new text, in the file at
/// `path`, those still as found; how many were written and how many were not.
fn write_lines(
    editor: &mut Editor,
    path: PathBuf,
    lines: Vec<(usize, String, String)>,
) -> std::io::Result<(usize, usize)> {
    let buffer_count = editor.buffers().len();
    let buffer = editor.find_file_noselect(&path)?;
    let (name, was_modified) = (buffer.name().to_owned(), buffer.is_modified());
    let text = buffer.text();
    let total = lines.len();
    let edits: Vec<_> = lines
        .into_iter()
        .filter_map(|(line, found, edited)| {
            let range = text.line_range(line)?;
            (line_text(text, line)? == found).then_some((range, edited))
        })
        .collect();
    // Opened only to be written: closed again once saved.
    let opened = editor.buffers().len() > buffer_count;
    let written = edits.len();
    if written > 0 {
        let buffer = editor.edit_file(FileEdits { path, edits })?;
        if !was_modified {
            buffer.save()?;
        }
    }
    if opened {
        editor.kill_buffer(&name);
    }
    Ok((written, total - written))
}

fn line_text(text: &Snapshot, line: usize) -> Option<String> {
    let range = text.line_range(line)?;
    Some(String::from_utf8_lossy(&text.bytes_in(range)).into_owned())
}
