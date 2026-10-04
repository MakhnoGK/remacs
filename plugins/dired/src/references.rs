//! References to files being renamed, such as Rust `mod` declarations, worked out by a
//! backend like `lsp` (`workspace/willRenameFiles`) and updated along with the renames.

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use remacs_core::{DiffLine, Editor, FileDiff, FileEdits};
use remacs_files::abbreviate_home;

use crate::listing::Listing;

/// Renames a backend is asked about; it answers with [`deliver_rename_edits`].
pub struct RenameRequest {
    pub id: u64,
    /// From and to, whole paths.
    pub renames: Vec<(PathBuf, PathBuf)>,
}

#[derive(Default)]
struct Requests {
    last_id: u64,
    queued: Vec<RenameRequest>,
}

/// What a listing being edited knows of the references its renames would update.
#[derive(Default)]
pub struct References {
    id: u64,
    /// The renames asked about, from and to.
    pub renames: Vec<(PathBuf, PathBuf)>,
    pub state: State,
}

#[derive(Default)]
pub enum State {
    /// No backend took the question, so there is nothing to wait for.
    #[default]
    Unanswered,
    /// A backend is working it out.
    Waiting,
    Known {
        edits: Vec<FileEdits>,
        diffs: Vec<FileDiff>,
    },
}

/// Asks backends what renaming `renames` would update; nothing to ask without renames.
pub fn ask(editor: &mut Editor, renames: Vec<(PathBuf, PathBuf)>) -> References {
    if renames.is_empty() {
        return References::default();
    }
    let requests = editor.plugin_state::<Requests>();
    requests.last_id += 1;
    let id = requests.last_id;
    requests.queued.push(RenameRequest {
        id,
        renames: renames.clone(),
    });
    // Backends' hooks may have run already in this pass.
    editor.notifier()();
    References {
        id,
        renames,
        state: State::Unanswered,
    }
}

/// The questions asked since the last call, for a backend to answer.
pub fn take_rename_requests(editor: &mut Editor) -> Vec<RenameRequest> {
    let requests = std::mem::take(&mut editor.plugin_state::<Requests>().queued);
    for references in editing_references(editor) {
        if requests.iter().any(|request| request.id == references.id) {
            references.state = State::Waiting;
        }
    }
    requests
}

/// Answers question `id` with the edits the renames call for, in offsets of the texts of
/// the buffers visiting the files, else of the files.
pub fn deliver_rename_edits(editor: &mut Editor, id: u64, edits: Vec<FileEdits>) {
    let Some(directory) = editor.buffers().iter().find_map(|buffer| {
        let listing = buffer.local::<Listing>()?;
        (listing.editing.as_ref()?.references.id == id).then(|| buffer.default_directory())?
    }) else {
        return;
    };
    let diffs = edits
        .iter()
        .filter_map(|file| diff(editor, file, directory))
        .collect();
    if let Some(references) = editing_references(editor).find(|references| references.id == id) {
        references.state = State::Known { edits, diffs };
    }
}

fn editing_references(editor: &mut Editor) -> impl Iterator<Item = &mut References> {
    editor.buffers_mut().iter_mut().filter_map(|buffer| {
        let editing = buffer.local_mut::<Listing>()?.editing.as_mut()?;
        Some(&mut editing.references)
    })
}

/// The lines `file` changes, before and after, under its name relative to `directory`.
fn diff(editor: &Editor, file: &FileEdits, directory: &Path) -> Option<FileDiff> {
    let text = text_of(editor, &file.path)?;
    let mut edits = file.edits.clone();
    edits.sort_by_key(|(range, _)| range.start);
    let mut lines = Vec::new();
    for (span, edits) in line_spans(&text, &edits) {
        let mut after = String::new();
        let mut copied = span.start;
        for (range, new_text) in edits {
            after.push_str(text.get(copied..range.start)?);
            after.push_str(new_text);
            copied = range.end;
        }
        after.push_str(text.get(copied..span.end)?);
        let before = text.get(span)?;
        lines.extend(
            before
                .split('\n')
                .map(|line| DiffLine::Removed(line.to_owned())),
        );
        lines.extend(
            after
                .split('\n')
                .map(|line| DiffLine::Added(line.to_owned())),
        );
    }
    let name = file.path.strip_prefix(directory).map_or_else(
        |_| abbreviate_home(&file.path),
        |relative| relative.display().to_string(),
    );
    Some(FileDiff { name, lines })
}

type Replacement = (Range<usize>, String);

/// The whole lines the sorted `edits` touch, merged where they meet, each with its edits.
fn line_spans<'a>(
    text: &str,
    edits: &'a [Replacement],
) -> Vec<(Range<usize>, Vec<&'a Replacement>)> {
    let mut spans: Vec<(Range<usize>, Vec<_>)> = Vec::new();
    for edit in edits {
        let (start, end) = (edit.0.start.min(text.len()), edit.0.end.min(text.len()));
        let line_start = text[..start].rfind('\n').map_or(0, |newline| newline + 1);
        let line_end = text[end..]
            .find('\n')
            .map_or(text.len(), |newline| end + newline);
        match spans.last_mut() {
            Some((span, edits)) if line_start <= span.end => {
                span.end = span.end.max(line_end);
                edits.push(edit);
            }
            _ => spans.push((line_start..line_end, vec![edit])),
        }
    }
    spans
}

/// The text the edits' offsets refer to: the buffer visiting `path`, else the file.
fn text_of(editor: &Editor, path: &Path) -> Option<String> {
    match editor
        .buffers()
        .iter()
        .find(|buffer| buffer.path() == Some(path))
    {
        Some(buffer) => Some(buffer.contents(0..buffer.text().len())),
        None => fs::read_to_string(path).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_share_the_span_of_the_lines_they_touch() {
        let text = "mod a;\nmod b;\n\nfn main() {}\n";
        let edits = vec![
            (4..5, "x".to_owned()),
            (5..5, "y".to_owned()),
            (11..12, "z".to_owned()),
            (14..14, "// new\n".to_owned()),
        ];
        let spans = line_spans(text, &edits);
        let shapes: Vec<_> = spans
            .iter()
            .map(|(span, edits)| (span.clone(), edits.len()))
            .collect();
        assert_eq!(shapes, [(0..6, 2), (7..13, 1), (14..14, 1)]);
    }
}
