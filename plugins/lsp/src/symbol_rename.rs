//! Renames the symbol at point everywhere it is used, for the rename plugin: its name
//! comes from `textDocument/prepareRename`, the changes from `textDocument/rename`.

use std::rc::Rc;

use lsp_types::PrepareRenameResponse;
use remacs_completion::is_word_char;
use remacs_core::{Editor, FileChange};
use remacs_rename::Request;
use remacs_text::Snapshot;

use crate::LanguageServer;
use crate::client::{Client, Outgoing, SymbolRenamed};
use crate::document::{Clients, code_untold, server_at};
use crate::file_rename::{self, Asker};
use crate::position::{Encoding, from_lsp};
use crate::workspace_edit::file_changes;

/// Requests held back while their server loads the project: rust-analyzer then finds
/// nothing to rename or cancels the rename. Each with the text it was made for.
#[derive(Default)]
struct Held {
    requests: Vec<(Request, Snapshot)>,
}

/// Hands answers over and sends the plugin's requests, new and held back.
pub(crate) fn sync(editor: &mut Editor, servers: &'static [LanguageServer]) {
    hand_over_answers(editor, servers);
    let held = std::mem::take(&mut editor.plugin_state::<Held>().requests);
    for (request, text) in held {
        send(editor, request, Some(text));
    }
    if code_untold(editor, editor.window_buffer()) {
        return;
    }
    for request in remacs_rename::take_requests(editor) {
        send(editor, request, None);
    }
}

/// Sends `request` unless its server is still loading the project; `held` is the text of
/// a request held back, which is dropped if the buffer changed since.
fn send(editor: &mut Editor, request: Request, held: Option<Snapshot>) {
    let (Request::Name { path, offset, .. } | Request::Rename { path, offset, .. }) = &request;
    let server = server_at(editor, path, *offset).ok_or("No language server for this buffer");
    let server = server.and_then(|server| match &held {
        Some(held) if server.2.edit_from(held).is_some() => {
            Err("The buffer changed while the language server was loading")
        }
        _ => Ok(server),
    });
    let (client, uri, text) = match server {
        Ok(server) => server,
        Err(failure) => {
            let failure = failure.to_owned();
            return match request {
                Request::Name { id, .. } => remacs_rename::deliver_name(editor, id, Err(failure)),
                Request::Rename { id, .. } => {
                    remacs_rename::deliver_changes(editor, id, Err(failure));
                }
            };
        }
    };
    if !client.is_settled() {
        if held.is_none() {
            editor.set_message("Waiting for the language server to load the project…");
        }
        editor.plugin_state::<Held>().requests.push((request, text));
        return;
    }
    client.send(match request {
        Request::Name { id, offset, .. } => Outgoing::PrepareRename { id, uri, text, offset },
        Request::Rename { id, offset, new_name, .. } => {
            Outgoing::Rename { id, uri, text, offset, new_name }
        }
    });
}

/// Changes that move files are completed with what the servers say moving them calls
/// for first.
fn hand_over_answers(editor: &mut Editor, servers: &'static [LanguageServer]) {
    let clients: Vec<Rc<Client>> =
        editor.plugin_state::<Clients>().running().map(|(_, client)| Rc::clone(client)).collect();
    for answer in clients.iter().flat_map(|client| client.take_symbol_renamed()) {
        match answer {
            SymbolRenamed::Name { id, name } => remacs_rename::deliver_name(editor, id, name),
            SymbolRenamed::Edit { id, edit, encoding } => {
                let changes = edit.and_then(|edit| match edit {
                    Some(edit) => file_changes(editor, edit, encoding),
                    None => Ok(Vec::new()),
                });
                let moves: Vec<_> = changes
                    .iter()
                    .flatten()
                    .filter_map(|change| match change {
                        FileChange::Rename { from, to } => Some((from.clone(), to.clone())),
                        _ => None,
                    })
                    .collect();
                match changes {
                    Ok(changes) if !moves.is_empty() => {
                        let asker = Asker::SymbolRename { id, changes };
                        file_rename::ask(editor, servers, &moves, asker);
                    }
                    changes => remacs_rename::deliver_changes(editor, id, changes),
                }
            }
        }
    }
}

/// The name a server offers to rename, from the range it gives or the word at `offset`.
pub(crate) fn name(
    response: PrepareRenameResponse,
    text: &Snapshot,
    offset: usize,
    encoding: Encoding,
) -> Option<String> {
    match response {
        PrepareRenameResponse::Range(range) => {
            let start = from_lsp(text, range.start, encoding);
            let end = from_lsp(text, range.end, encoding).max(start);
            let name = String::from_utf8_lossy(&text.bytes_in(start..end)).into_owned();
            (!name.is_empty()).then_some(name)
        }
        PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. } => Some(placeholder),
        PrepareRenameResponse::DefaultBehavior { .. } => word_at(text, offset),
    }
}

/// The word `offset` is in or just after.
pub(crate) fn word_at(text: &Snapshot, offset: usize) -> Option<String> {
    let (line, column) = text.position(offset);
    let range = text.line_range(line)?;
    let line = String::from_utf8_lossy(&text.bytes_in(range)).into_owned();
    let (before, after) = line.split_at_checked(column)?;
    let start = before
        .char_indices()
        .rev()
        .take_while(|&(_, character)| is_word_char(character))
        .last()
        .map_or(column, |(index, _)| index);
    let end = after
        .char_indices()
        .find(|&(_, character)| !is_word_char(character))
        .map_or(line.len(), |(index, _)| column + index);
    (start < end).then(|| line[start..end].to_owned())
}

#[cfg(test)]
mod tests {
    use lsp_types::{Position, Range};
    use remacs_text::Text;

    use super::*;

    #[test]
    fn the_word_at_point_is_the_one_it_is_in_or_just_after() {
        let text = Text::from("let ж_1 = x.len();\n").snapshot();
        assert_eq!(word_at(&text, 6).as_deref(), Some("ж_1"));
        assert_eq!(word_at(&text, 4).as_deref(), Some("ж_1"));
        assert_eq!(word_at(&text, 3).as_deref(), Some("let"));
        assert_eq!(word_at(&text, 12).as_deref(), Some("x"));
        assert_eq!(word_at(&text, 13).as_deref(), Some("len"));
        assert_eq!(word_at(&text, 10), None);
    }

    #[test]
    fn the_name_is_the_range_the_server_gives_unless_it_offers_one() {
        let text = Text::from("fn helper() {}\n").snapshot();
        let range = Range::new(Position::new(0, 3), Position::new(0, 9));
        let name = |response| name(response, &text, 5, Encoding::Utf16);
        assert_eq!(name(PrepareRenameResponse::Range(range)).as_deref(), Some("helper"));
        let placeholder =
            PrepareRenameResponse::RangeWithPlaceholder { range, placeholder: "aid".into() };
        assert_eq!(name(placeholder).as_deref(), Some("aid"));
        let default = PrepareRenameResponse::DefaultBehavior { default_behavior: true };
        assert_eq!(name(default).as_deref(), Some("helper"));
    }
}
