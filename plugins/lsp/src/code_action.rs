use lsp_types::{
    CodeActionKind, CodeActionOrCommand, DocumentChangeOperation, DocumentChanges, OneOf, TextEdit,
    Uri, WorkspaceEdit,
};
use remacs_actions::Action;
use remacs_core::{CommandResult, Editor, FileEdits};

use crate::client::{Client, Outgoing};
use crate::position::{Encoding, from_lsp, text_of};
use crate::uri;

/// The actions as the menu shows them, in the server's order.
pub(crate) fn menu(actions: &[CodeActionOrCommand]) -> Vec<Action> {
    actions
        .iter()
        .map(|action| match action {
            CodeActionOrCommand::Command(command) => Action {
                title: command.title.clone(),
                kind: "command".into(),
                preferred: false,
            },
            CodeActionOrCommand::CodeAction(action) => Action {
                title: action.title.clone(),
                kind: action.kind.as_ref().map(kind_label).unwrap_or_default(),
                preferred: action.is_preferred == Some(true),
            },
        })
        .collect()
}

/// `quickfix` → `quick fix`, `refactor.extract` → `extract`.
fn kind_label(kind: &CodeActionKind) -> String {
    let kind = kind.as_str();
    let label = match kind {
        "quickfix" => "quick fix",
        "source.organizeImports" => "organize imports",
        "source.fixAll" => "fix all",
        _ => kind.rsplit('.').next().unwrap_or(kind),
    };
    label.to_owned()
}

/// Applies the action's edit and runs its command. An action that lacks both, or keeps
/// what its edit is computed from in `data`, is resolved first when `resolve`.
pub(crate) fn carry_out(
    editor: &mut Editor,
    client: &Client,
    id: u64,
    action: CodeActionOrCommand,
    encoding: Encoding,
    resolve: bool,
) -> CommandResult {
    let action = match action {
        CodeActionOrCommand::Command(command) => {
            client.execute(id, command);
            return Ok(());
        }
        CodeActionOrCommand::CodeAction(action) => action,
    };
    let unresolved = action.edit.is_none() && (action.data.is_some() || action.command.is_none());
    if resolve && unresolved {
        client.send(Outgoing::ResolveAction {
            id,
            action: Box::new(action),
        });
        return Ok(());
    }
    if action.edit.is_none() && action.command.is_none() {
        return Err("The code action does nothing".into());
    }
    if let Some(edit) = action.edit {
        apply(editor, id, edit, encoding)?;
    }
    if let Some(command) = action.command {
        client.execute(id, command);
    }
    Ok(())
}

pub(crate) fn apply(
    editor: &mut Editor,
    id: u64,
    edit: WorkspaceEdit,
    encoding: Encoding,
) -> CommandResult {
    let changes = file_edits(editor, edit, encoding)?;
    remacs_actions::apply(editor, id, changes)
}

/// The edits per file, in offsets of the texts the server has. `documentChanges` wins
/// over `changes`, as the protocol says.
pub(crate) fn file_edits(
    editor: &Editor,
    edit: WorkspaceEdit,
    encoding: Encoding,
) -> Result<Vec<FileEdits>, String> {
    let mut by_file: Vec<(Uri, Vec<TextEdit>)> = Vec::new();
    match edit.document_changes {
        Some(DocumentChanges::Edits(edits)) => {
            by_file.extend(edits.into_iter().map(|edit| {
                let edits = edit.edits.into_iter().map(text_edit).collect();
                (edit.text_document.uri, edits)
            }));
        }
        Some(DocumentChanges::Operations(operations)) => {
            for operation in operations {
                let DocumentChangeOperation::Edit(edit) = operation else {
                    return Err("Creating, renaming or deleting files is not supported".into());
                };
                let edits = edit.edits.into_iter().map(text_edit).collect();
                by_file.push((edit.text_document.uri, edits));
            }
        }
        None => by_file.extend(edit.changes.into_iter().flatten()),
    }
    by_file
        .into_iter()
        .map(|(uri, edits)| {
            let path = uri::to_path(&uri).ok_or_else(|| format!("Not a file: {}", uri.as_str()))?;
            let text = text_of(editor, &path)
                .ok_or_else(|| format!("{}: cannot be read", path.display()))?;
            let edits = edits
                .into_iter()
                .map(|edit| {
                    let start = from_lsp(&text, edit.range.start, encoding);
                    let end = from_lsp(&text, edit.range.end, encoding).max(start);
                    (start..end, edit.new_text)
                })
                .collect();
            Ok(FileEdits { path, edits })
        })
        .collect()
}

fn text_edit(edit: OneOf<TextEdit, lsp_types::AnnotatedTextEdit>) -> TextEdit {
    match edit {
        OneOf::Left(edit) => edit,
        OneOf::Right(annotated) => annotated.text_edit,
    }
}
