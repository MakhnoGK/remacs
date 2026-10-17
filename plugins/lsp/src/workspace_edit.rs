//! The changes to files a server's workspace edit makes, in offsets of the texts it has.

use std::path::{Path, PathBuf};

use lsp_types::{
    AnnotatedTextEdit, DocumentChangeOperation, DocumentChanges, OneOf,
    OptionalVersionedTextDocumentIdentifier, ResourceOp, TextDocumentEdit, TextEdit, Uri,
    WorkspaceEdit,
};
use remacs_core::{Editor, FileChange, FileEdits, embedded_file};
use remacs_text::{Snapshot, Text};

use crate::position::{Encoding, from_lsp, text_of};
use crate::uri;

/// The edits per file, for edits that only change text.
pub(crate) fn file_edits(
    editor: &Editor,
    edit: WorkspaceEdit,
    encoding: Encoding,
) -> Result<Vec<FileEdits>, String> {
    file_changes(editor, edit, encoding)?
        .into_iter()
        .map(|change| match change {
            FileChange::Edit(edits) => Ok(edits),
            _ => Err("Creating or renaming files is not supported here".into()),
        })
        .collect()
}

/// The changes in the order they are made. `documentChanges` wins over `changes`, as the
/// protocol says. Edits to a file renamed or created before them refer to the text it
/// will have by then.
pub(crate) fn file_changes(
    editor: &Editor,
    edit: WorkspaceEdit,
    encoding: Encoding,
) -> Result<Vec<FileChange>, String> {
    let operations = match edit.document_changes {
        Some(DocumentChanges::Edits(edits)) => edits
            .into_iter()
            .map(DocumentChangeOperation::Edit)
            .collect(),
        Some(DocumentChanges::Operations(operations)) => operations,
        None => edit
            .changes
            .into_iter()
            .flatten()
            .map(|(uri, edits)| {
                DocumentChangeOperation::Edit(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
                    edits: edits.into_iter().map(OneOf::Left).collect(),
                })
            })
            .collect(),
    };
    let mut origins = Origins::default();
    let mut changes = Vec::new();
    for operation in operations {
        let change = match operation {
            DocumentChangeOperation::Edit(edit) => {
                let path = path_of(&edit.text_document.uri)?;
                let text = origins
                    .text(editor, &path)
                    .ok_or_else(|| format!("{}: cannot be read", path.display()))?;
                let edits = edit
                    .edits
                    .into_iter()
                    .map(|edit| {
                        let edit = text_edit(edit);
                        let start = from_lsp(&text, edit.range.start, encoding);
                        let end = from_lsp(&text, edit.range.end, encoding).max(start);
                        (start..end, edit.new_text)
                    })
                    .collect();
                // Offsets in embedded code's document are the file's.
                let path = embedded_file(&path).unwrap_or(path);
                FileChange::Edit(FileEdits { path, edits })
            }
            DocumentChangeOperation::Op(ResourceOp::Create(create)) => {
                let path = path_of(&create.uri)?;
                origins.moved.push((path.clone(), None));
                FileChange::Create(path)
            }
            DocumentChangeOperation::Op(ResourceOp::Rename(rename)) => {
                let (from, to) = (path_of(&rename.old_uri)?, path_of(&rename.new_uri)?);
                origins.moved.push((to.clone(), Some(from.clone())));
                FileChange::Rename { from, to }
            }
            DocumentChangeOperation::Op(ResourceOp::Delete(_)) => {
                return Err("Deleting files is not supported".into());
            }
        };
        changes.push(change);
    }
    Ok(changes)
}

/// Where the files made so far come from: renamed from a path, or created.
#[derive(Default)]
struct Origins {
    /// Newest last.
    moved: Vec<(PathBuf, Option<PathBuf>)>,
}

impl Origins {
    /// The text `path` will have before the edits to it, as the file it is renamed from
    /// has now, or empty when created.
    fn text(&self, editor: &Editor, path: &Path) -> Option<Snapshot> {
        let mut path = path.to_path_buf();
        for (made, origin) in self.moved.iter().rev() {
            let Ok(rest) = path.strip_prefix(made) else {
                continue;
            };
            match origin {
                Some(from) if rest.as_os_str().is_empty() => path = from.clone(),
                Some(from) => path = from.join(rest),
                None => return Some(Text::default().snapshot()),
            }
        }
        text_of(editor, &path)
    }
}

fn path_of(uri: &Uri) -> Result<PathBuf, String> {
    uri::to_path(uri).ok_or_else(|| format!("Not a file: {}", uri.as_str()))
}

fn text_edit(edit: OneOf<TextEdit, AnnotatedTextEdit>) -> TextEdit {
    match edit {
        OneOf::Left(edit) => edit,
        OneOf::Right(annotated) => annotated.text_edit,
    }
}
