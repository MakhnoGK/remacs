use std::cmp::Reverse;
use std::ops::Range;
use std::path::{Path, PathBuf};

use remacs_core::{Buffer, CommandResult, Editor};

use crate::menu::{Awaited, state};

/// Edits to one file, in offsets of the text of the buffer visiting it, else of the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEdits {
    pub path: PathBuf,
    /// Replacements that do not overlap; inserts at one offset keep their order.
    pub edits: Vec<(Range<usize>, String)>,
}

/// Makes the edits of the action chosen from list `id`, opening files that are not
/// visited yet; they are left unsaved. Edits that come after another command are
/// refused, since the text they were made for may have changed.
pub fn apply(editor: &mut Editor, id: u64, changes: Vec<FileEdits>) -> CommandResult {
    if state(editor).awaited != Some(Awaited::Apply(id)) {
        return Err("Code action dropped: another command ran since".into());
    }
    let shown = editor.window_buffer().path().map(Path::to_path_buf);
    let mut others = Vec::new();
    for FileEdits { path, edits } in changes {
        let buffer = editor
            .find_file_noselect(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        buffer.add_undo_boundary();
        replace_all(buffer, edits);
        if buffer.path() != shown.as_deref() {
            others.push(buffer.name().to_owned());
        }
    }
    if !others.is_empty() {
        editor.set_message(format!("Also edited {}", others.join(", ")));
    }
    Ok(())
}

/// Tells the user that the action chosen from list `id` could not be carried out.
pub fn failed(editor: &mut Editor, id: u64, message: &str) {
    if state(editor).awaited == Some(Awaited::Apply(id)) {
        editor.set_message(format!("Code action failed: {message}"));
    }
}

/// Makes `edits`, all in offsets of the current text, last first so the others stay
/// valid; point stays on its character.
fn replace_all(buffer: &mut Buffer, edits: Vec<(Range<usize>, String)>) {
    let mut edits: Vec<_> = edits.into_iter().enumerate().collect();
    edits.sort_by_key(|(index, (range, _))| Reverse((range.start, *index)));
    let mut point = buffer.point();
    let len = buffer.text().len();
    for (_, (range, text)) in edits {
        let range = range.start.min(len)..range.end.min(len);
        point = shift(point, &range, text.len());
        buffer.delete(range.clone());
        buffer.set_point(range.start);
        buffer.insert(&text);
    }
    buffer.set_point(point);
}

/// Where `offset` ends up when `range` is replaced with `inserted` bytes: text inserted
/// at it goes after it, like an Emacs marker.
fn shift(offset: usize, range: &Range<usize>, inserted: usize) -> usize {
    if offset <= range.start {
        offset
    } else if offset >= range.end {
        offset - range.len() + inserted
    } else {
        range.start
    }
}
