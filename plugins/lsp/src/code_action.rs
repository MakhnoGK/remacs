use lsp_types::{CodeActionKind, CodeActionOrCommand, WorkspaceEdit};
use remacs_actions::Action;
use remacs_core::{CommandResult, Editor};

use crate::client::{Client, Outgoing};
use crate::position::Encoding;
use crate::workspace_edit::file_edits;

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

/// Whether `action` is of `kind` or one of its subkinds, as `source.fixAll.oxc` is of
/// `source.fixAll`.
pub(crate) fn is_of_kind(action: &CodeActionOrCommand, kind: &str) -> bool {
    let CodeActionOrCommand::CodeAction(action) = action else {
        return false;
    };
    action.kind.as_ref().is_some_and(|own| {
        let own = own.as_str();
        own.strip_prefix(kind)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    })
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
