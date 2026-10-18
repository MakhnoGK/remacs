//! Asks a file's servers for its symbols (`textDocument/documentSymbol`), for xref's
//! `imenu`, and hands back what they all found, in the servers' order.

use std::path::Path;
use std::rc::Rc;

use lsp_types::{DocumentSymbol, DocumentSymbolResponse};
use remacs_core::Editor;
use remacs_xref::{DocumentSymbolRequest, Symbol};

use crate::LanguageServer;
use crate::client::{Client, Outgoing};
use crate::document::Clients;
use crate::file_rename::clients_for;
use crate::symbols::{FoundSymbol, place, symbol_kind};
use crate::uri;

/// The newest question and each server's answer, once it gave one.
#[derive(Default)]
struct Asked {
    id: u64,
    parts: Vec<(Rc<Client>, Option<Vec<Symbol>>)>,
}

/// Hands answers over and sends xref's newest question.
pub(crate) fn sync(editor: &mut Editor, servers: &'static [LanguageServer]) {
    collect_answers(editor);
    if let Some(request) = remacs_xref::take_document_symbol_requests(editor).pop() {
        ask(editor, servers, request);
    }
}

/// What a server answered, its nested symbols each with what it is declared in.
pub(crate) fn found(response: Option<DocumentSymbolResponse>, path: &Path) -> Vec<FoundSymbol> {
    match response {
        None => Vec::new(),
        Some(DocumentSymbolResponse::Flat(symbols)) => symbols
            .into_iter()
            .map(|symbol| FoundSymbol {
                name: symbol.name,
                kind: symbol_kind(symbol.kind),
                container: symbol.container_name.filter(|name| !name.is_empty()),
                path: path.to_path_buf(),
                position: symbol.location.range.start,
            })
            .collect(),
        Some(DocumentSymbolResponse::Nested(symbols)) => {
            let mut found = Vec::new();
            flatten(symbols, None, path, &mut found);
            found
        }
    }
}

fn flatten(
    symbols: Vec<DocumentSymbol>,
    container: Option<&str>,
    path: &Path,
    found: &mut Vec<FoundSymbol>,
) {
    for symbol in symbols {
        found.push(FoundSymbol {
            name: symbol.name.clone(),
            kind: symbol_kind(symbol.kind),
            container: container.map(str::to_owned),
            path: path.to_path_buf(),
            position: symbol.selection_range.start,
        });
        flatten(
            symbol.children.unwrap_or_default(),
            Some(&symbol.name),
            path,
            found,
        );
    }
}

/// Asks every server of the file, starting them if needed; with none, answers at once.
fn ask(editor: &mut Editor, servers: &'static [LanguageServer], request: DocumentSymbolRequest) {
    let clients = clients_for(editor, servers, &request.path);
    if clients.is_empty() {
        *editor.plugin_state::<Asked>() = Asked::default();
        let missing = "No language server for this file".to_owned();
        return remacs_xref::deliver_document_symbols(editor, request.id, Err(missing));
    }
    let uri = uri::from_path(&request.path);
    for client in &clients {
        client.send(Outgoing::DocumentSymbols {
            id: request.id,
            uri: uri.clone(),
        });
    }
    *editor.plugin_state::<Asked>() = Asked {
        id: request.id,
        parts: clients.into_iter().map(|client| (client, None)).collect(),
    };
}

/// Delivers once every server asked has answered.
fn collect_answers(editor: &mut Editor) {
    let clients: Vec<Rc<Client>> = editor
        .plugin_state::<Clients>()
        .running()
        .map(|(_, client)| Rc::clone(client))
        .collect();
    for client in clients {
        for found in client.take_document_symbols() {
            let asked = editor.plugin_state::<Asked>();
            let is_asked = asked.id == found.id
                && asked
                    .parts
                    .iter()
                    .any(|(part, _)| Rc::ptr_eq(part, &client));
            if !is_asked {
                continue;
            }
            let symbols = place(editor, found.symbols, found.encoding);
            let asked = editor.plugin_state::<Asked>();
            if let Some((_, answer)) = asked
                .parts
                .iter_mut()
                .find(|(part, _)| Rc::ptr_eq(part, &client))
            {
                *answer = Some(symbols);
            }
            let all: Option<Vec<Symbol>> = asked
                .parts
                .iter()
                .map(|(_, answer)| answer.clone())
                .collect::<Option<Vec<_>>>()
                .map(|answers| answers.concat());
            if let Some(symbols) = all {
                let id = asked.id;
                remacs_xref::deliver_document_symbols(editor, id, Ok(symbols));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use remacs_core::SymbolKind;
    use serde_json::json;

    use super::*;

    #[test]
    fn nested_symbols_are_listed_after_what_they_are_in() {
        let range = |line: u32| json!({ "start": { "line": line, "character": 4 }, "end": { "line": line, "character": 9 } });
        let nested = json!([{
            "name": "Buffer", "kind": 23, "range": range(1), "selectionRange": range(1),
            "children": [
                { "name": "point", "kind": 6, "range": range(3), "selectionRange": range(3) },
            ],
        }]);
        let found = found(
            serde_json::from_value(nested).ok(),
            Path::new("/p/buffer.rs"),
        );
        let listed: Vec<_> = found
            .iter()
            .map(|symbol| {
                (
                    symbol.name.as_str(),
                    symbol.kind,
                    symbol.container.as_deref(),
                    symbol.position.line,
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                ("Buffer", Some(SymbolKind::Type), None, 1),
                ("point", Some(SymbolKind::Method), Some("Buffer"), 3),
            ]
        );
    }
}
