//! Walks call and type hierarchies (`callHierarchy/*`, `typeHierarchy/*`) for xref's
//! hierarchy pane: the items at point from the server of the code there, then each item's
//! callers, callees, supertypes or subtypes from the server that gave it.

use std::rc::Rc;

use lsp_types::{Position, Uri};
use remacs_core::{Editor, embedded_file};
use remacs_xref::{HierarchyItem, HierarchyRequest, Location, Relation};
use serde_json::Value;

use crate::client::{Client, Outgoing, Walked};
use crate::document::{self, Clients};
use crate::position::{from_lsp, text_of};
use crate::uri;

/// The items of the shown hierarchy, by xref's key, with the server that gave each.
#[derive(Default)]
struct Items(Vec<(Rc<Client>, Value)>);

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
            let buffer = editor.window_buffer();
            let at = buffer.path().and_then(|path| document::server_at(editor, path, offset));
            let Some((client, uri, text)) = at else {
                let missing = "No language server for this file".to_owned();
                return remacs_xref::deliver_hierarchy(editor, id, Err(missing));
            };
            // A new tree: the items of the one before are asked about no more.
            editor.plugin_state::<Items>().0.clear();
            client.send(Outgoing::PrepareHierarchy { id, hierarchy, uri, text, offset });
        }
        HierarchyRequest::Expand { id, key, relation } => {
            let Some((client, item)) = editor.plugin_state::<Items>().0.get(key as usize).cloned()
            else {
                return remacs_xref::deliver_hierarchy(editor, id, Ok(Vec::new()));
            };
            client.send(Outgoing::ExpandHierarchy { id, relation, item });
        }
    }
}

/// The items a server answered, kept to ask about again and placed in the texts it
/// knows; one it cannot place is left out.
fn place(editor: &mut Editor, client: &Rc<Client>, walked: Walked) -> (u64, Vec<HierarchyItem>) {
    let items = walked.items.into_iter().filter_map(|item| {
        let name = item["name"].as_str()?.to_owned();
        let detail = item["detail"].as_str().unwrap_or_default().to_owned();
        let uri: Uri = serde_json::from_value(item["uri"].clone()).ok()?;
        let position: Position =
            serde_json::from_value(item["selectionRange"]["start"].clone()).ok()?;
        let path = uri::to_path(&uri)?;
        let offset = from_lsp(&text_of(editor, &path)?, position, walked.encoding);
        let path = embedded_file(&path).unwrap_or(path);
        let location = Location { path, offset };
        Some((name, detail, location, position.line as usize, item))
    });
    let items: Vec<_> = items.collect();
    let kept = &mut editor.plugin_state::<Items>().0;
    let items = items
        .into_iter()
        .map(|(name, detail, location, line, item)| {
            kept.push((Rc::clone(client), item));
            HierarchyItem { key: kept.len() as u64 - 1, name, detail, location, line }
        })
        .collect();
    (walked.id, items)
}

/// The items of an answer: the callers' or callees' of a call hierarchy's, else the
/// items themselves; none from a failed one.
pub(crate) fn items(result: Value, relation: Option<Relation>) -> Vec<Value> {
    let Value::Array(answered) = result else {
        return Vec::new();
    };
    let field = match relation {
        Some(Relation::Callers) => "from",
        Some(Relation::Callees) => "to",
        _ => return answered,
    };
    answered.into_iter().map(|mut call| call[field].take()).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn calls_give_their_callers_or_callees_and_types_themselves() {
        let caller = json!({ "name": "main" });
        let callee = json!({ "name": "speak" });
        let calls = json!([{ "from": caller, "to": callee, "fromRanges": [] }]);
        assert_eq!(items(calls.clone(), Some(Relation::Callers)), vec![caller.clone()]);
        assert_eq!(items(calls, Some(Relation::Callees)), vec![callee]);
        assert_eq!(items(json!([caller]), Some(Relation::Subtypes)), vec![caller.clone()]);
        assert_eq!(items(json!([caller]), None), vec![caller]);
        assert!(items(Value::Null, Some(Relation::Callers)).is_empty());
    }
}
