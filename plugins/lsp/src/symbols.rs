//! Asks the project's servers for its symbols by name (`workspace/symbol`), for xref's
//! `xref-find-apropos`, and hands back what they all found.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use lsp_types::{OneOf, Position, SymbolKind as LspKind, WorkspaceSymbolResponse};
use remacs_core::{Editor, SymbolKind};
use remacs_text::Snapshot;
use remacs_xref::{Location, Symbol, SymbolRequest};

use crate::LanguageServer;
use crate::client::{Client, Outgoing};
use crate::document::Clients;
use crate::file_rename::clients_for;
use crate::position::{Encoding, from_lsp, text_of};
use crate::uri;

/// A symbol as a server reported it; only the editor has the file's text to place it.
pub(crate) struct FoundSymbol {
    pub name: String,
    pub kind: Option<SymbolKind>,
    pub container: Option<String>,
    pub path: PathBuf,
    pub position: Position,
}

/// The newest question, which replaces the one before.
#[derive(Default)]
struct Asked {
    question: Option<Question>,
}

struct Question {
    id: u64,
    query: String,
    /// One per server asked, in the servers' order.
    parts: Vec<Part>,
}

struct Part {
    client: Rc<Client>,
    /// What it found, once it answered.
    symbols: Option<Vec<Symbol>>,
    /// It answered while still loading the project, so it is asked again once done.
    ask_again: bool,
}

/// Hands answers over and sends xref's newest question.
pub(crate) fn sync(editor: &mut Editor, servers: &'static [LanguageServer]) {
    collect_answers(editor);
    ask_settled_servers_again(editor);
    if let Some(request) = remacs_xref::take_symbol_requests(editor).pop() {
        ask(editor, servers, request);
    }
}

/// What a server answered, without what cannot be placed, such as a symbol known only by
/// its file.
pub(crate) fn found(response: Option<WorkspaceSymbolResponse>) -> Vec<FoundSymbol> {
    let reported: Vec<_> = match response {
        None => Vec::new(),
        Some(WorkspaceSymbolResponse::Flat(symbols)) => symbols
            .into_iter()
            .map(|symbol| {
                let location = symbol.location;
                let place = (location.uri, location.range.start);
                (symbol.name, symbol.kind, symbol.container_name, Some(place))
            })
            .collect(),
        Some(WorkspaceSymbolResponse::Nested(symbols)) => symbols
            .into_iter()
            .map(|symbol| {
                let place = match symbol.location {
                    OneOf::Left(location) => Some((location.uri, location.range.start)),
                    OneOf::Right(_) => None,
                };
                (symbol.name, symbol.kind, symbol.container_name, place)
            })
            .collect(),
    };
    reported
        .into_iter()
        .filter_map(|(name, kind, container, place)| {
            let (uri, position) = place?;
            Some(FoundSymbol {
                name,
                kind: symbol_kind(kind),
                container: container.filter(|container| !container.is_empty()),
                path: uri::to_file(&uri)?,
                position,
            })
        })
        .collect()
}

/// Asks every server of the project, starting those of the file searched from if needed;
/// with none, answers at once.
fn ask(editor: &mut Editor, servers: &'static [LanguageServer], request: SymbolRequest) {
    let clients = clients_for(editor, servers, &request.from);
    if clients.is_empty() {
        editor.plugin_state::<Asked>().question = None;
        let missing = "No language server for this project".to_owned();
        return remacs_xref::deliver_symbols(editor, request.id, Err(missing));
    }
    for client in &clients {
        client.send(Outgoing::Symbols {
            id: request.id,
            query: request.query.clone(),
        });
    }
    let parts = clients
        .into_iter()
        .map(|client| Part {
            client,
            symbols: None,
            ask_again: false,
        })
        .collect();
    editor.plugin_state::<Asked>().question = Some(Question {
        id: request.id,
        query: request.query,
        parts,
    });
}

/// Delivers once every server asked has answered, in the servers' order, and again
/// whenever one that was loading answers anew.
fn collect_answers(editor: &mut Editor) {
    let clients: Vec<Rc<Client>> = editor
        .plugin_state::<Clients>()
        .running()
        .map(|(_, client)| Rc::clone(client))
        .collect();
    for client in clients {
        for found in client.take_found() {
            let asked = editor.plugin_state::<Asked>();
            let is_asked = asked.question.as_ref().is_some_and(|question| {
                question.id == found.id
                    && question
                        .parts
                        .iter()
                        .any(|part| Rc::ptr_eq(&part.client, &client))
            });
            if !is_asked {
                continue;
            }
            let symbols = place(editor, found.symbols, found.encoding);
            let Some(question) = editor.plugin_state::<Asked>().question.as_mut() else {
                continue;
            };
            if let Some(part) = question
                .parts
                .iter_mut()
                .find(|part| Rc::ptr_eq(&part.client, &client))
            {
                part.symbols = Some(symbols);
                part.ask_again = !found.settled;
            }
            let all: Option<Vec<&Vec<Symbol>>> = question
                .parts
                .iter()
                .map(|part| part.symbols.as_ref())
                .collect();
            let Some(all) = all else {
                continue;
            };
            let symbols = all.into_iter().flatten().cloned().collect();
            let id = question.id;
            remacs_xref::deliver_symbols(editor, id, Ok(symbols));
        }
    }
}

fn ask_settled_servers_again(editor: &mut Editor) {
    let Some(question) = editor.plugin_state::<Asked>().question.as_mut() else {
        return;
    };
    for part in &mut question.parts {
        if part.ask_again && part.client.is_settled() {
            part.ask_again = false;
            part.client.send(Outgoing::Symbols {
                id: question.id,
                query: question.query.clone(),
            });
        }
    }
}

/// Offsets in the texts the servers know, each file read once.
fn place(editor: &Editor, found: Vec<FoundSymbol>, encoding: Encoding) -> Vec<Symbol> {
    let mut texts: HashMap<PathBuf, Option<Snapshot>> = HashMap::new();
    found
        .into_iter()
        .filter_map(|symbol| {
            let text = texts
                .entry(symbol.path.clone())
                .or_insert_with(|| text_of(editor, &symbol.path))
                .as_ref()?;
            let offset = from_lsp(text, symbol.position, encoding);
            Some(Symbol {
                name: symbol.name,
                kind: symbol.kind,
                container: symbol.container,
                line: symbol.position.line as usize,
                location: Location {
                    path: symbol.path,
                    offset,
                },
            })
        })
        .collect()
}

fn symbol_kind(kind: LspKind) -> Option<SymbolKind> {
    Some(match kind {
        LspKind::FUNCTION | LspKind::OPERATOR => SymbolKind::Function,
        LspKind::METHOD | LspKind::CONSTRUCTOR => SymbolKind::Method,
        LspKind::FIELD | LspKind::PROPERTY | LspKind::KEY => SymbolKind::Field,
        LspKind::VARIABLE => SymbolKind::Variable,
        LspKind::CONSTANT => SymbolKind::Constant,
        LspKind::ENUM_MEMBER => SymbolKind::EnumMember,
        LspKind::CLASS | LspKind::STRUCT | LspKind::ENUM | LspKind::TYPE_PARAMETER => {
            SymbolKind::Type
        }
        LspKind::INTERFACE => SymbolKind::Interface,
        LspKind::MODULE | LspKind::NAMESPACE | LspKind::PACKAGE | LspKind::FILE => {
            SymbolKind::Module
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn reported(
        response: serde_json::Value,
    ) -> Vec<(String, Option<SymbolKind>, Option<String>, u32)> {
        found(serde_json::from_value(response).ok())
            .into_iter()
            .map(|symbol| {
                (
                    symbol.name,
                    symbol.kind,
                    symbol.container,
                    symbol.position.line,
                )
            })
            .collect()
    }

    #[test]
    fn flat_and_nested_answers_are_read_alike() {
        let range = json!({ "start": { "line": 4, "character": 7 }, "end": { "line": 4, "character": 13 } });
        let flat = json!([
            { "name": "Buffer", "kind": 23, "location": { "uri": "file:///p/buffer.rs", "range": range } },
            { "name": "point", "kind": 6, "containerName": "Buffer", "location": { "uri": "file:///p/buffer.rs", "range": range } },
        ]);
        assert_eq!(
            reported(flat),
            [
                ("Buffer".into(), Some(SymbolKind::Type), None, 4),
                (
                    "point".into(),
                    Some(SymbolKind::Method),
                    Some("Buffer".into()),
                    4
                ),
            ]
        );
        let nested = json!([
            { "name": "main", "kind": 12, "containerName": "", "location": { "uri": "file:///p/main.rs", "range": range } },
            { "name": "lazy", "kind": 13, "location": { "uri": "file:///p/lazy.rs" } },
        ]);
        assert_eq!(
            reported(nested),
            [("main".into(), Some(SymbolKind::Function), None, 4)]
        );
        assert!(reported(json!(null)).is_empty());
    }
}
