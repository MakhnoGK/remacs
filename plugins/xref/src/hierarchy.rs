//! Call and type hierarchies, like IntelliJ's: who calls the function at point or what it
//! calls, the supertypes of the type at point or its subtypes. A tree in a pane under the
//! text, each row opened as it is asked for; backends such as `lsp` take its requests
//! and deliver the items.

use std::collections::HashMap;
use std::mem;
use std::ops::RangeInclusive;
use std::path::Path;
use std::rc::Rc;

use remacs_core::{
    Command, CommandResult, Editor, Pane, PaneBody, PaneButton, PaneColumn, PaneRow, Section, Tone,
};
use serde::Deserialize;

use crate::definitions::{Location, jump};

pub(crate) const NAME: &str = "Hierarchy";
pub const CALL_HIERARCHY: &str = "xref-call-hierarchy";
pub const TYPE_HIERARCHY: &str = "xref-type-hierarchy";
const NEXT: &str = "xref-hierarchy-next";
const PREVIOUS: &str = "xref-hierarchy-previous";
const TOGGLE: &str = "xref-hierarchy-toggle";
const GO: &str = "xref-hierarchy-go";
const FLIP: &str = "xref-hierarchy-flip";
const CLOSE: &str = "xref-hierarchy-close";
const LEAVE: &str = "xref-hierarchy-leave";
const PANE_HEIGHTS: RangeInclusive<f32> = 160.0..=1200.0;

#[rustfmt::skip]
pub(crate) const COMMANDS: &[Command] = &[
    Command::new(CALL_HIERARCHY, "Show the callers of the function at point, and theirs", call_hierarchy),
    Command::new(TYPE_HIERARCHY, "Show the supertypes of the type at point, and theirs", type_hierarchy),
    Command::new(NEXT, "Select the next row of the hierarchy", next),
    Command::new(PREVIOUS, "Select the previous row of the hierarchy", previous),
    Command::new(TOGGLE, "Open the selected row of the hierarchy, or close it", toggle),
    Command::new(GO, "Go to the selected row of the hierarchy", go),
    Command::new(FLIP, "Show the other way: callees for callers, subtypes for supertypes", flip),
    Command::new(CLOSE, "Close the hierarchy", close),
    Command::new(LEAVE, "Give the keys back to the text", leave),
];

/// While the pane has the keys.
pub(crate) const FOCUSED_BINDINGS: &[(&str, &str)] = &[
    ("C-n", NEXT),
    ("<down>", NEXT),
    ("C-p", PREVIOUS),
    ("<up>", PREVIOUS),
    ("TAB", TOGGLE),
    ("RET", GO),
    ("d", FLIP),
    ("q", CLOSE),
    ("C-g", LEAVE),
    ("ESC", LEAVE),
];

const PANE_HINTS: &[(&str, &str)] = &[("C-x o", "Pane"), ("q", "Close")];
const COLUMN_HINTS: &[(&str, &str)] = &[("TAB", "Expand"), ("↵", "Go to"), ("d", "Flip")];

/// `xref.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct XrefSettings {
    /// Pixels the hierarchy pane takes under the text.
    pub hierarchy_pane_height: f32,
}

impl Default for XrefSettings {
    fn default() -> Self {
        Self { hierarchy_pane_height: 320.0 }
    }
}

impl Section for XrefSettings {
    const NAME: &str = "xref";

    fn check(&self) -> Result<(), String> {
        if PANE_HEIGHTS.contains(&self.hierarchy_pane_height) {
            return Ok(());
        }
        Err(format!(
            "hierarchy-pane-height must be {} to {}",
            PANE_HEIGHTS.start(),
            PANE_HEIGHTS.end()
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hierarchy {
    Calls,
    Types,
}

/// Which way a hierarchy goes from an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Callers,
    Callees,
    Supertypes,
    Subtypes,
}

impl Relation {
    fn flipped(self) -> Self {
        match self {
            Relation::Callers => Relation::Callees,
            Relation::Callees => Relation::Callers,
            Relation::Supertypes => Relation::Subtypes,
            Relation::Subtypes => Relation::Supertypes,
        }
    }

    /// Its name, and the flip button's label.
    fn words(self) -> (&'static str, &'static str) {
        match self {
            Relation::Callers => ("Callers", "Show callees"),
            Relation::Callees => ("Callees", "Show callers"),
            Relation::Supertypes => ("Supertypes", "Show subtypes"),
            Relation::Subtypes => ("Subtypes", "Show supertypes"),
        }
    }
}

impl Hierarchy {
    fn first(self) -> Relation {
        match self {
            Hierarchy::Calls => Relation::Callers,
            Hierarchy::Types => Relation::Supertypes,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Hierarchy::Calls => "call hierarchy",
            Hierarchy::Types => "type hierarchy",
        }
    }
}

/// A function or type of a hierarchy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HierarchyItem {
    /// The backend's, to ask about the item again.
    pub key: u64,
    pub name: String,
    /// E.g. a function's signature, or its module.
    pub detail: String,
    pub location: Location,
    /// Of the location, from 0.
    pub line: usize,
    /// Of a caller or callee, the calls it is a caller or callee by, in the caller, with
    /// their lines; rows go to the first.
    pub calls: Vec<(Location, usize)>,
}

impl HierarchyItem {
    /// Where its row goes: its first call, else the item itself; and that line.
    fn target(&self) -> (&Location, usize) {
        self.calls.first().map_or((&self.location, self.line), |(location, line)| (location, *line))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HierarchyRequest {
    /// The items at `offset` of the window buffer, which the tree starts from.
    Prepare { id: u64, offset: usize, hierarchy: Hierarchy },
    /// The items `relation` of the item `key`.
    Expand { id: u64, key: u64, relation: Relation },
}

struct Node {
    item: HierarchyItem,
    open: bool,
    /// `None` until they are known.
    children: Option<Vec<Node>>,
}

struct Tree {
    relation: Relation,
    roots: Vec<Node>,
    selected: usize,
}

/// What an answer is for: a new tree, or the children of the node at a path.
enum Awaited {
    Roots(Hierarchy),
    Children(Vec<usize>),
}

#[derive(Default)]
struct State {
    next_id: u64,
    requests: Vec<HierarchyRequest>,
    awaited: HashMap<u64, Awaited>,
    tree: Option<Tree>,
    settings: XrefSettings,
}

fn state(editor: &mut Editor) -> &mut State {
    editor.plugin_state()
}

pub(crate) fn read_settings(editor: &mut Editor) {
    state(editor).settings = editor.settings_section::<XrefSettings>();
}

pub fn take_hierarchy_requests(editor: &mut Editor) -> Vec<HierarchyRequest> {
    mem::take(&mut state(editor).requests)
}

fn send(editor: &mut Editor, awaited: Awaited, request: impl FnOnce(u64) -> HierarchyRequest) {
    let state = state(editor);
    state.next_id += 1;
    state.awaited.insert(state.next_id, awaited);
    state.requests.push(request(state.next_id));
}

fn call_hierarchy(editor: &mut Editor) -> CommandResult {
    prepare(editor, Hierarchy::Calls)
}

fn type_hierarchy(editor: &mut Editor) -> CommandResult {
    prepare(editor, Hierarchy::Types)
}

fn prepare(editor: &mut Editor, hierarchy: Hierarchy) -> CommandResult {
    if editor.minibuffer().is_some() {
        return Err("Symbols are looked up in buffers only".into());
    }
    let offset = editor.window_buffer().point();
    state(editor).awaited.retain(|_, awaited| matches!(awaited, Awaited::Children(_)));
    send(editor, Awaited::Roots(hierarchy), |id| HierarchyRequest::Prepare {
        id,
        offset,
        hierarchy,
    });
    Ok(())
}

/// Answers request `id`: starts the tree, or fills a row's children. A failure is said,
/// and a row it was for closes, to be asked again when opened.
pub fn deliver_hierarchy(editor: &mut Editor, id: u64, items: Result<Vec<HierarchyItem>, String>) {
    let Some(awaited) = state(editor).awaited.remove(&id) else {
        return;
    };
    let items = match items {
        Ok(items) => items,
        Err(error) => {
            if let Awaited::Children(path) = awaited
                && let Some(found) = tree_node(editor, &path)
            {
                found.open = false;
            }
            return editor.set_error(error);
        }
    };
    match awaited {
        Awaited::Roots(hierarchy) if items.is_empty() => {
            editor.set_error(format!("Nothing at point has a {}", hierarchy.name()));
        }
        Awaited::Roots(hierarchy) => {
            let roots = items.into_iter().map(node).collect();
            let relation = hierarchy.first();
            let state = state(editor);
            state.awaited.retain(|_, awaited| matches!(awaited, Awaited::Roots(_)));
            state.tree = Some(Tree { relation, roots, selected: 0 });
            open(editor, &[0]);
            editor.select_pane(NAME);
            editor.focus_pane(true);
        }
        Awaited::Children(path) => {
            if let Some(found) = tree_node(editor, &path) {
                found.children = Some(items.into_iter().map(node).collect());
            }
        }
    }
}

fn node(item: HierarchyItem) -> Node {
    Node { item, open: false, children: None }
}

fn tree_node<'a>(editor: &'a mut Editor, path: &[usize]) -> Option<&'a mut Node> {
    let tree = state(editor).tree.as_mut()?;
    let (first, rest) = path.split_first()?;
    let mut found = tree.roots.get_mut(*first)?;
    for &index in rest {
        found = found.children.as_mut()?.get_mut(index)?;
    }
    Some(found)
}

/// Opens the row at `path`, asking for its children the first time.
fn open(editor: &mut Editor, path: &[usize]) {
    let Some(relation) = state(editor).tree.as_ref().map(|tree| tree.relation) else {
        return;
    };
    let Some(found) = tree_node(editor, path) else {
        return;
    };
    found.open = true;
    if found.children.is_none() {
        let key = found.item.key;
        let awaited = Awaited::Children(path.to_vec());
        send(editor, awaited, |id| HierarchyRequest::Expand { id, key, relation });
    }
}

/// The rows shown: each node with its path, its children under it while open.
fn rows(tree: &Tree) -> Vec<(Vec<usize>, &Node)> {
    fn walk<'a>(nodes: &'a [Node], path: &[usize], rows: &mut Vec<(Vec<usize>, &'a Node)>) {
        for (index, node) in nodes.iter().enumerate() {
            let path = [path, &[index]].concat();
            rows.push((path.clone(), node));
            if let Some(children) = node.children.as_deref().filter(|_| node.open) {
                walk(children, &path, rows);
            }
        }
    }
    let mut rows = Vec::new();
    walk(&tree.roots, &[], &mut rows);
    rows
}

/// The selected row's path, whether it is open, and where its item is.
fn selected(editor: &mut Editor) -> Option<(Vec<usize>, bool, Location)> {
    let tree = state(editor).tree.as_ref()?;
    let rows = rows(tree);
    let (path, node) = rows.get(tree.selected.min(rows.len().checked_sub(1)?))?;
    Some((path.clone(), node.open, node.item.target().0.clone()))
}

fn step(editor: &mut Editor, forward: bool) -> CommandResult {
    let tree = state(editor).tree.as_mut().ok_or("No hierarchy")?;
    let last = rows(tree).len().saturating_sub(1);
    tree.selected = if forward {
        (tree.selected + 1).min(last)
    } else {
        tree.selected.saturating_sub(1).min(last)
    };
    Ok(())
}

fn next(editor: &mut Editor) -> CommandResult {
    step(editor, true)
}

fn previous(editor: &mut Editor) -> CommandResult {
    step(editor, false)
}

fn toggle(editor: &mut Editor) -> CommandResult {
    let (path, is_open, _) = selected(editor).ok_or("No hierarchy")?;
    if is_open {
        tree_node(editor, &path).ok_or("No hierarchy")?.open = false;
    } else {
        open(editor, &path);
    }
    Ok(())
}

/// Goes to the selected row's item, giving the keys back to the text.
fn go(editor: &mut Editor) -> CommandResult {
    let (_, _, location) = selected(editor).ok_or("No hierarchy")?;
    editor.focus_pane(false);
    jump(editor, &location)
}

/// Shows the other way from the same items, e.g. callees for callers.
fn flip(editor: &mut Editor) -> CommandResult {
    let state = state(editor);
    let tree = state.tree.as_mut().ok_or("No hierarchy")?;
    tree.relation = tree.relation.flipped();
    tree.selected = 0;
    for root in &mut tree.roots {
        *root = node(root.item.clone());
    }
    state.awaited.retain(|_, awaited| matches!(awaited, Awaited::Roots(_)));
    open(editor, &[0]);
    Ok(())
}

fn close(editor: &mut Editor) -> CommandResult {
    let state = state(editor);
    state.tree = None;
    state.awaited.retain(|_, awaited| matches!(awaited, Awaited::Roots(_)));
    editor.focus_pane(false);
    Ok(())
}

fn leave(editor: &mut Editor) -> CommandResult {
    editor.focus_pane(false);
    Ok(())
}

pub(crate) fn has_keys(editor: &Editor) -> bool {
    editor.pane_has_keys(NAME)
}

pub(crate) fn is_shown(editor: &Editor) -> bool {
    editor.find_plugin_state::<State>().is_some_and(|state| state.tree.is_some())
}

pub(crate) fn pane(editor: &Editor) -> Option<Pane> {
    let shown = editor.find_plugin_state::<State>()?;
    let tree = shown.tree.as_ref()?;
    let rows = rows(tree);
    let focused = has_keys(editor);
    let (relation, flip_label) = tree.relation.words();
    let asking = shown.awaited.values().any(|awaited| matches!(awaited, Awaited::Children(_)));
    let pane_rows = rows
        .iter()
        .enumerate()
        .map(|(index, (path, node))| PaneRow {
            depth: path.len() - 1,
            open: match &node.children {
                Some(children) if children.is_empty() => None,
                _ => Some(node.open),
            },
            name: node.item.name.clone(),
            kind: node.item.detail.clone(),
            detail: place(&node.item),
            action: Some(Rc::new(move |editor: &mut Editor| {
                state(editor).tree.as_mut().ok_or("No hierarchy")?.selected = index;
                go(editor)
            })),
            ..PaneRow::default()
        })
        .collect();
    let button = |label, keys, command: &'static str| PaneButton {
        label,
        keys,
        enabled: true,
        action: Rc::new(move |editor: &mut Editor| {
            let run = editor.commands().get(command).ok_or("No such command")?.run;
            run(editor)
        }),
    };
    let column = PaneColumn {
        title: relation,
        keys: "",
        width: None,
        rows: pane_rows,
        selected: focused.then(|| tree.selected.min(rows.len().saturating_sub(1))),
        focused,
        empty: format!("No {}", relation.to_lowercase()),
        hints: COLUMN_HINTS,
    };
    let root = tree.roots.first()?;
    Some(Pane {
        count: None,
        height: shown.settings.hierarchy_pane_height,
        title: format!("{relation} of {}", root.item.name),
        subtitle: place(&root.item),
        state: if asking {
            ("Asking".into(), Tone::Warning)
        } else {
            (relation.into(), Tone::Accent)
        },
        tabs: None,
        buttons: vec![button(flip_label, "d", FLIP), button("Close", "q", CLOSE)],
        body: PaneBody::Columns(vec![column]),
        hints: PANE_HINTS,
    })
}

/// `main.rs:12`, and `×3` after the first call when there are more.
fn place(item: &HierarchyItem) -> String {
    let (location, line) = item.target();
    let name = location.path.file_name().map(Path::new).unwrap_or(&location.path);
    let place = format!("{}:{}", name.display(), line + 1);
    // shortcut: only the first call is gone to; list each once callers that call many
    // times need them apart.
    match item.calls.len() {
        0 | 1 => place,
        calls => format!("{place} ×{calls}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_default_file_reads_as_the_defaults() {
        remacs_core::assert_default_file::<super::XrefSettings>();
    }
}
