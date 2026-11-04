//! Walks call and type hierarchies (`callHierarchy/*`, `typeHierarchy/*`) for xref's
//! hierarchy pane: the items at point from the server of the code there, then each item's
//! callers, callees, supertypes or subtypes from the server that gave it.

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use lsp_types::{Position, Uri};
use remacs_core::{Editor, embedded_file};
use remacs_xref::{HierarchyItem, HierarchyRequest, Location, Relation};
use serde_json::Value;

use crate::client::{Client, Outgoing, Walked};
use crate::document::{self, Clients};
use crate::position::{Encoding, from_lsp, text_of};
use crate::uri;

/// The items of the shown hierarchy, by xref's key, with the server that gave each.
#[derive(Default)]
struct Items {
    kept: Vec<(Rc<Client>, Value)>,
    /// Of each request for an item's callees, that item's file, where the calls are.
    callers: HashMap<u64, Uri>,
}

/// Hands over what the servers answered, then sends xref's requests.
pub(crate) fn sync(editor: &mut Editor) {
    let clients: Vec<Rc<Client>> =
        editor.plugin_state::<Clients>().running().map(|(_, client)| Rc::clone(client)).collect();
    for client in clients {
        for walked in client.take_walked() {
            let (id, items) = place(editor, &client, walked);
            remacs_xref::deliver_hierarchy(editor, id, Ok(items));
        }
    }
    for request in remacs_xref::take_hierarchy_requests(editor) {
        ask(editor, request);
    }
}

fn ask(editor: &mut Editor, request: HierarchyRequest) {
    match request {
        HierarchyRequest::Prepare { id, offset, hierarchy } => {
            let path = editor.window_buffer().path().map(Path::to_path_buf).unwrap_or_default();
            let walks = |client: &Client| client.may_walk(hierarchy);
            let (client, uri, text) = match document::server_at(editor, &path, offset, walks) {
                Ok(server) => server,
                Err(refusing) => {
                    let error = refusing.map_or_else(
                        || "No language server for this file".to_owned(),
                        |server| format!("{server} cannot show a {}", hierarchy.name()),
                    );
                    return remacs_xref::deliver_hierarchy(editor, id, Err(error));
                }
            };
            // A new tree: the items of the one before are asked about no more.
            *editor.plugin_state::<Items>() = Items::default();
            client.send(Outgoing::PrepareHierarchy { id, hierarchy, uri, text, offset });
        }
        HierarchyRequest::Expand { id, key, relation } => {
            let items = editor.plugin_state::<Items>();
            let Some((client, item)) = items.kept.get(key as usize).cloned() else {
                return remacs_xref::deliver_hierarchy(editor, id, Ok(Vec::new()));
            };
            if relation == Relation::Callees
                && let Ok(file) = serde_json::from_value(item["uri"].clone())
            {
                items.callers.insert(id, file);
            }
            client.send(Outgoing::ExpandHierarchy { id, relation, item });
        }
    }
}

/// The items a server answered, kept to ask about again and placed, with their calls, in
/// the texts it knows; one it cannot place is left out.
fn place(editor: &mut Editor, client: &Rc<Client>, walked: Walked) -> (u64, Vec<HierarchyItem>) {
    let callers_file = editor.plugin_state::<Items>().callers.remove(&walked.id);
    let encoding = walked.encoding;
    let items = walked.items.into_iter().filter_map(|(item, calls)| {
        let name = item["name"].as_str()?.to_owned();
        let detail = item["detail"].as_str().unwrap_or_default().to_owned();
        let file: Uri = serde_json::from_value(item["uri"].clone()).ok()?;
        let position: Position =
            serde_json::from_value(item["selectionRange"]["start"].clone()).ok()?;
        let (location, line) = locate(editor, &file, [position], encoding).pop()?;
        let calls = locate(editor, callers_file.as_ref().unwrap_or(&file), calls, encoding);
        Some((name, detail, location, line, calls, item))
    });
    let items: Vec<_> = items.collect();
    let kept = &mut editor.plugin_state::<Items>().kept;
    let items = items
        .into_iter()
        .map(|(name, detail, location, line, calls, item)| {
            kept.push((Rc::clone(client), item));
            HierarchyItem { key: kept.len() as u64 - 1, name, detail, location, line, calls }
        })
        .collect();
    (walked.id, items)
}

/// Where `positions` of `file` are in the text the server knows, with their lines.
fn locate(
    editor: &Editor,
    file: &Uri,
    positions: impl IntoIterator<Item = Position>,
    encoding: Encoding,
) -> Vec<(Location, usize)> {
    let Some(path) = uri::to_path(file) else {
        return Vec::new();
    };
    let Some(text) = text_of(editor, &path) else {
        return Vec::new();
    };
    let shown = embedded_file(&path).unwrap_or(path);
    let place = |position: Position| {
        let location =
            Location { path: shown.clone(), offset: from_lsp(&text, position, encoding) };
        (location, position.line as usize)
    };
    positions.into_iter().map(place).collect()
}

/// The items of an answer, each with where its calls are: the callers or callees of a
/// call hierarchy's, else the items themselves; none from a failed one.
pub(crate) fn items(result: Value, relation: Option<Relation>) -> Vec<(Value, Vec<Position>)> {
    let Value::Array(answered) = result else {
        return Vec::new();
    };
    let field = match relation {
        Some(Relation::Callers) => "from",
        Some(Relation::Callees) => "to",
        _ => return answered.into_iter().map(|item| (item, Vec::new())).collect(),
    };
    let starts = |ranges: Value| -> Vec<Position> {
        let ranges: Vec<lsp_types::Range> = serde_json::from_value(ranges).unwrap_or_default();
        ranges.into_iter().map(|range| range.start).collect()
    };
    answered
        .into_iter()
        .map(|mut call| (call[field].take(), starts(call["fromRanges"].take())))
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn calls_give_their_callers_or_callees_with_where_they_call() {
        let caller = json!({ "name": "main" });
        let callee = json!({ "name": "speak" });
        let at = |line, character| json!({ "start": { "line": line, "character": character }, "end": { "line": line, "character": character + 5 } });
        let calls = json!([{ "from": caller, "to": callee, "fromRanges": [at(3, 4), at(5, 8)] }]);
        let sites = vec![Position::new(3, 4), Position::new(5, 8)];
        assert_eq!(
            items(calls.clone(), Some(Relation::Callers)),
            vec![(caller.clone(), sites.clone())]
        );
        assert_eq!(items(calls, Some(Relation::Callees)), vec![(callee, sites)]);
        let types = vec![(caller.clone(), Vec::new())];
        assert_eq!(items(json!([caller]), Some(Relation::Subtypes)), types);
        assert_eq!(items(json!([caller]), None), types);
        assert!(items(Value::Null, Some(Relation::Callers)).is_empty());
    }
}
