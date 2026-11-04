//! Walks call and type hierarchies (`callHierarchy/*`, `typeHierarchy/*`) for xref's
//! hierarchy pane: the items at point from the server of the code there, then each item's
//! callers, callees, supertypes or subtypes from the server that gave it.

use std::collections::HashMap;
use std::path::Path;
use std::rc::{Rc, Weak};

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
struct Items {
    kept: Vec<(Weak<Client>, Value)>,
    /// Of each request for an item's callees, that item's file, where the calls are.
    callers: HashMap<u64, Uri>,
    /// The request for a new tree's roots, whose items replace those kept; until they
    /// come, the tree shown still opens.
    roots: Option<u64>,
    /// Requests not answered yet, by the server asked.
    asked: HashMap<u64, Weak<Client>>,
}

/// Said of a request whose server was restarted or stopped before it answered, or of an
/// item it gave.
const STOPPED: &str = "Its language server stopped: open the hierarchy again";

/// Hands over what the servers answered, then sends xref's requests.
pub(crate) fn sync(editor: &mut Editor) {
    let clients: Vec<Rc<Client>> =
        editor.plugin_state::<Clients>().running().map(|(_, client)| Rc::clone(client)).collect();
    for client in clients {
        for walked in client.take_walked() {
            let id = walked.id;
            editor.plugin_state::<Items>().asked.remove(&id);
            let items = place(editor, &client, walked);
            remacs_xref::deliver_hierarchy(editor, id, items);
        }
    }
    // Their answers would never be taken, leaving the rows asking for good.
    for id in editor.plugin_state::<Items>().orphaned() {
        remacs_xref::deliver_hierarchy(editor, id, Err(STOPPED.to_owned()));
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
            let items = editor.plugin_state::<Items>();
            items.roots = Some(id);
            items.asked.insert(id, Rc::downgrade(&client));
            client.send(Outgoing::PrepareHierarchy { id, hierarchy, uri, text, offset });
        }
        HierarchyRequest::Expand { id, key, relation } => {
            let items = editor.plugin_state::<Items>();
            let kept = items.kept.get(key as usize).cloned();
            // Held weakly, so that a server restarted or stopped exits rather than lives on
            // for the tree.
            let Some((client, item)) =
                kept.and_then(|(client, item)| Some((client.upgrade()?, item)))
            else {
                return remacs_xref::deliver_hierarchy(editor, id, Err(STOPPED.to_owned()));
            };
            if let Some(file) = calls_in(relation, &item) {
                items.callers.insert(id, file);
            }
            items.asked.insert(id, Rc::downgrade(&client));
            client.send(Outgoing::ExpandHierarchy { id, relation, item });
        }
    }
}

/// The file the calls of the items `relation` of `item` are in, when it is not theirs:
/// `item`'s own for its callees, since it is the caller.
fn calls_in(relation: Relation, item: &Value) -> Option<Uri> {
    (relation == Relation::Callees).then(|| serde_json::from_value(item["uri"].clone()).ok())?
}

/// The items a server answered, kept to ask about again and placed, with their calls, in
/// the texts it knows; one it cannot place is left out.
fn place(
    editor: &mut Editor,
    client: &Rc<Client>,
    walked: Walked,
) -> Result<Vec<HierarchyItem>, String> {
    let callers_file = editor.plugin_state::<Items>().callers.remove(&walked.id);
    let answered = walked.items?;
    let mut texts = HashMap::new();
    let mut locate = |file: &Uri, positions: Vec<Position>| {
        let path = uri::to_path(file)?;
        let text = texts.entry(path.clone()).or_insert_with(|| text_of(editor, &path)).clone()?;
        let shown = embedded_file(&path).unwrap_or(path);
        let place = |position: Position| {
            let offset = from_lsp(&text, position, walked.encoding);
            (Location { path: shown.clone(), offset }, position.line as usize)
        };
        Some(positions.into_iter().map(place).collect::<Vec<_>>())
    };
    let (answered, placed): (Vec<_>, Vec<_>) = answered
        .into_iter()
        .filter_map(|(item, calls)| {
            let name = item["name"].as_str()?.to_owned();
            let detail = item["detail"].as_str().unwrap_or_default().to_owned();
            let file: Uri = serde_json::from_value(item["uri"].clone()).ok()?;
            let position: Position =
                serde_json::from_value(item["selectionRange"]["start"].clone()).ok()?;
            let (location, line) = locate(&file, vec![position])?.pop()?;
            let calls = locate(callers_file.as_ref().unwrap_or(&file), calls).unwrap_or_default();
            Some((item, (name, detail, location, line, calls)))
        })
        .unzip();
    let keys = editor.plugin_state::<Items>().keep(walked.id, &Rc::downgrade(client), answered);
    let placed =
        keys.into_iter().zip(placed).map(|(key, (name, detail, location, line, calls))| {
            HierarchyItem { key, name, detail, location, line, calls }
        });
    Ok(placed.collect())
}

impl Items {
    /// Keeps the `answered` items of request `id`, asked of `client`, and gives their
    /// keys; a new tree's roots replace those kept, unless there are none.
    fn keep(&mut self, id: u64, client: &Weak<Client>, answered: Vec<Value>) -> Vec<u64> {
        if self.roots == Some(id) && !answered.is_empty() {
            self.kept.clear();
            self.roots = None;
        }
        let first = self.kept.len() as u64;
        self.kept.extend(answered.into_iter().map(|item| (Weak::clone(client), item)));
        (first..self.kept.len() as u64).collect()
    }

    /// Takes the requests whose server is gone without answering them.
    fn orphaned(&mut self) -> Vec<u64> {
        let gone: Vec<u64> = (self.asked.iter())
            .filter(|(_, client)| client.strong_count() == 0)
            .map(|(&id, _)| id)
            .collect();
        for id in &gone {
            self.asked.remove(id);
            self.callers.remove(id);
        }
        gone
    }
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

    /// A new tree's roots replace the items kept, unless there are none, so the tree
    /// shown still opens after one fails to start.
    #[test]
    fn a_new_trees_roots_replace_the_items_kept_unless_there_are_none() {
        let mut items = Items { roots: Some(2), ..Items::default() };
        let item = |name: &str| json!({ "name": name });
        let kept =
            |items: &Items| items.kept.iter().map(|(_, item)| item.clone()).collect::<Vec<_>>();
        assert_eq!(items.keep(1, &Weak::new(), vec![item("old"), item("child")]), [0, 1]);
        assert_eq!(items.keep(2, &Weak::new(), Vec::new()), Vec::<u64>::new());
        assert_eq!(kept(&items), [item("old"), item("child")]);
        assert_eq!(items.roots, Some(2));
        assert_eq!(items.keep(2, &Weak::new(), vec![item("new")]), [0]);
        assert_eq!(kept(&items), [item("new")]);
        assert_eq!(items.keep(3, &Weak::new(), vec![item("its child")]), [1]);
    }

    /// A request whose server is gone is taken once, with what was kept for its answer.
    #[test]
    fn requests_of_a_server_gone_are_orphaned_once() {
        let mut items = Items::default();
        items.asked.insert(1, Weak::new());
        items.callers.insert(1, "file:///p/main.rs".parse().unwrap());
        assert_eq!(items.orphaned(), [1]);
        assert!(items.asked.is_empty() && items.callers.is_empty());
        assert!(items.orphaned().is_empty());
    }

    /// A callee's calls are in the item asked about, the caller; a caller's in its own.
    #[test]
    fn calls_are_in_the_caller() {
        let item = json!({ "name": "main", "uri": "file:///p/main.rs" });
        let main: Uri = "file:///p/main.rs".parse().unwrap();
        assert_eq!(calls_in(Relation::Callees, &item), Some(main));
        for relation in [Relation::Callers, Relation::Supertypes, Relation::Subtypes] {
            assert_eq!(calls_in(relation, &item), None);
        }
    }
}
