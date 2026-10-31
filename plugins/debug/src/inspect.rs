//! What a paused program holds: its threads, the variables of the selected frame and the
//! values of the watched expressions, loaded from the adapter as they are opened.

use std::collections::{HashMap, HashSet};

use remacs_core::Editor;
use serde_json::{Value, json};

use crate::session::{Address, Link, debugger, find_link, request, show_frame};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Column {
    #[default]
    Frames,
    Variables,
    Watches,
}

pub const COLUMNS: [Column; 3] = [Column::Frames, Column::Variables, Column::Watches];

impl Column {
    pub fn index(self) -> usize {
        self as usize
    }
}

/// How the paused program is looked at, kept from one stop to the next.
#[derive(Default)]
pub struct View {
    /// The column that has the keys while the pane has them.
    pub focus: Column,
    /// The selected row of each column.
    pub selected: [usize; 3],
    /// Paths of the variables, scopes and watches open, e.g. `Local/self/items`.
    pub open: HashSet<String>,
    /// Values shown before the program last went on, by path, to tell those that changed.
    pub previous: HashMap<String, String>,
    /// Expressions evaluated at every stop.
    pub watches: Vec<String>,
}

#[derive(Default)]
pub struct Inspected {
    /// Ids and names.
    pub threads: Vec<(i64, String)>,
    pub scopes: Vec<Scope>,
    /// The variables in each container loaded, by its reference.
    pub children: HashMap<i64, Vec<Variable>>,
    /// Of each watch, in order; none until evaluated.
    pub watches: Vec<Option<Result<Variable, String>>>,
}

pub struct Scope {
    pub name: String,
    pub reference: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub value: String,
    /// Its type, if the adapter says.
    pub kind: String,
    /// Of its fields or elements; 0 when it has none.
    pub reference: i64,
}

impl Variable {
    fn from(value: &Value, name: &str, value_key: &str) -> Self {
        Self {
            name: name.to_owned(),
            value: value[value_key].as_str().unwrap_or_default().to_owned(),
            kind: value["type"].as_str().unwrap_or_default().to_owned(),
            reference: value["variablesReference"].as_i64().unwrap_or_default(),
        }
    }
}

/// A row of the variables, in the order shown.
pub enum Entry<'a> {
    /// A scope other than the first, whose variables show at the top.
    Scope { name: &'a str, path: String, open: bool, reference: i64 },
    Variable {
        variable: &'a Variable,
        path: String,
        depth: usize,
        open: bool,
        /// Its container's reference, to set its value.
        container: i64,
    },
}

impl Inspected {
    /// The variables as shown: the first scope's at the top, the other scopes as
    /// headings, each open one's contents under it.
    pub fn entries<'a>(&'a self, open: &HashSet<String>) -> Vec<Entry<'a>> {
        let mut entries = Vec::new();
        for (index, scope) in self.scopes.iter().enumerate() {
            let path = scope.name.clone();
            let depth = usize::from(index > 0);
            if index > 0 {
                let is_open = open.contains(&path);
                entries.push(Entry::Scope {
                    name: &scope.name,
                    path: path.clone(),
                    open: is_open,
                    reference: scope.reference,
                });
                if !is_open {
                    continue;
                }
            }
            self.walk(scope.reference, &path, depth, open, &mut entries);
        }
        entries
    }

    /// What is inside `reference` as shown, e.g. an open watch's fields.
    pub fn entries_under<'a>(
        &'a self,
        reference: i64,
        path: &str,
        depth: usize,
        open: &HashSet<String>,
    ) -> Vec<Entry<'a>> {
        let mut entries = Vec::new();
        self.walk(reference, path, depth, open, &mut entries);
        entries
    }

    fn walk<'a>(
        &'a self,
        container: i64,
        path: &str,
        depth: usize,
        open: &HashSet<String>,
        entries: &mut Vec<Entry<'a>>,
    ) {
        for variable in self.children.get(&container).into_iter().flatten() {
            let path = format!("{path}/{}", variable.name);
            let is_open = variable.reference > 0 && open.contains(&path);
            entries.push(Entry::Variable {
                variable,
                path: path.clone(),
                depth,
                open: is_open,
                container,
            });
            if is_open {
                self.walk(variable.reference, &path, depth + 1, open, entries);
            }
        }
    }

    /// The values of everything loaded, by path.
    pub fn values(&self) -> HashMap<String, String> {
        let mut values = HashMap::new();
        for scope in &self.scopes {
            self.collect(scope.reference, &scope.name, &mut values);
        }
        for watch in self.watches.iter().flatten().flatten() {
            let path = watch_path(&watch.name);
            values.insert(path.clone(), watch.value.clone());
            self.collect(watch.reference, &path, &mut values);
        }
        values
    }

    fn collect(&self, container: i64, path: &str, values: &mut HashMap<String, String>) {
        for variable in self.children.get(&container).into_iter().flatten() {
            let path = format!("{path}/{}", variable.name);
            values.insert(path.clone(), variable.value.clone());
            if variable.reference > 0 {
                self.collect(variable.reference, &path, values);
            }
        }
    }

    /// The variables of the first scope, e.g. a function's locals.
    pub fn locals(&self) -> &[Variable] {
        self.scopes
            .first()
            .and_then(|scope| self.children.get(&scope.reference))
            .map_or(&[], Vec::as_slice)
    }
}

pub fn watch_path(expression: &str) -> String {
    format!("watch:{expression}")
}

fn frame_id(link: &Link) -> Option<i64> {
    link.frames.get(link.frame).map(|frame| frame.id)
}

/// Loads the threads, the selected frame's variables and the watches.
pub fn load(editor: &mut Editor, address: Address) {
    request(editor, address, "threads", json!({}), |editor, address, result| {
        let threads = result
            .ok()
            .and_then(|body| body["threads"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|thread| {
                let name = thread["name"].as_str().unwrap_or_default().to_owned();
                (thread["id"].as_i64().unwrap_or_default(), name)
            })
            .collect();
        if let Some(link) = find_link(editor, address) {
            link.inspected.threads = threads;
        }
    });
    load_frame(editor, address);
}

/// Loads the selected frame's variables and evaluates the watches in it.
pub fn load_frame(editor: &mut Editor, address: Address) {
    let Some(frame) = find_link(editor, address).and_then(|link| frame_id(link)) else {
        return;
    };
    request(editor, address, "scopes", json!({ "frameId": frame }), |editor, address, result| {
        let scopes: Vec<Scope> = result
            .ok()
            .and_then(|body| body["scopes"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|scope| Scope {
                name: scope["name"].as_str().unwrap_or_default().to_owned(),
                reference: scope["variablesReference"].as_i64().unwrap_or_default(),
            })
            .collect();
        let open = debugger(editor).view.open.clone();
        let wanted: Vec<(i64, String)> = scopes
            .iter()
            .enumerate()
            .filter(|(index, scope)| *index == 0 || open.contains(&scope.name))
            .map(|(_, scope)| (scope.reference, scope.name.clone()))
            .collect();
        let Some(link) = find_link(editor, address) else {
            return;
        };
        link.inspected.scopes = scopes;
        link.inspected.children.clear();
        for (reference, path) in wanted {
            load_variables(editor, address, reference, path);
        }
    });
    evaluate_watches(editor, address);
}

/// Loads the contents of `reference`, and of those inside it that are open.
pub fn load_variables(editor: &mut Editor, address: Address, reference: i64, path: String) {
    let arguments = json!({ "variablesReference": reference });
    request(editor, address, "variables", arguments, move |editor, address, result| {
        let variables: Vec<Variable> = result
            .ok()
            .and_then(|body| body["variables"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|variable| {
                let name = variable["name"].as_str().unwrap_or_default();
                Variable::from(variable, name, "value")
            })
            .collect();
        let open = &debugger(editor).view.open;
        let wanted: Vec<(i64, String)> = variables
            .iter()
            .filter(|variable| variable.reference > 0)
            .map(|variable| (variable.reference, format!("{path}/{}", variable.name)))
            .filter(|(_, path)| open.contains(path))
            .collect();
        let Some(link) = find_link(editor, address) else {
            return;
        };
        link.inspected.children.insert(reference, variables);
        for (reference, path) in wanted {
            load_variables(editor, address, reference, path);
        }
    });
}

/// Evaluates every watch in the selected frame.
pub fn evaluate_watches(editor: &mut Editor, address: Address) {
    let expressions = debugger(editor).view.watches.clone();
    let Some(link) = find_link(editor, address) else {
        return;
    };
    let frame = frame_id(link);
    link.inspected.watches = vec![None; expressions.len()];
    if link.stopped.is_none() {
        return;
    }
    for (index, expression) in expressions.into_iter().enumerate() {
        let arguments = json!({ "expression": expression, "frameId": frame, "context": "watch" });
        request(editor, address, "evaluate", arguments, move |editor, address, result| {
            let result = result.map(|body| Variable::from(&body, &expression, "result"));
            let open = result.as_ref().is_ok_and(|watch| {
                watch.reference > 0 && debugger(editor).view.open.contains(&watch_path(&expression))
            });
            let Some(link) = find_link(editor, address) else {
                return;
            };
            if let Some(slot) = link.inspected.watches.get_mut(index) {
                *slot = Some(result.clone());
            }
            if let (true, Ok(watch)) = (open, result) {
                load_variables(editor, address, watch.reference, watch_path(&expression));
            }
        });
    }
}

/// Opens what `path` names, loading its contents first if needed, or closes it.
pub fn toggle_open(editor: &mut Editor, address: Address, path: String, reference: i64) {
    let view = &mut debugger(editor).view;
    if view.open.remove(&path) {
        return;
    }
    view.open.insert(path.clone());
    let loaded = find_link(editor, address)
        .is_some_and(|link| link.inspected.children.contains_key(&reference));
    if !loaded {
        load_variables(editor, address, reference, path);
    }
}

/// Makes frame `index` the one shown and inspected.
pub fn select_frame(editor: &mut Editor, address: Address, index: usize) {
    let Some(link) = find_link(editor, address) else {
        return;
    };
    if index >= link.frames.len() {
        return;
    }
    link.frame = index;
    show_frame(editor, address);
    load_frame(editor, address);
}

/// Sets a variable in `container` to `value`, then loads the frame again.
pub fn set_value(
    editor: &mut Editor,
    address: Address,
    container: i64,
    name: String,
    value: String,
) {
    let arguments = json!({ "variablesReference": container, "name": name, "value": value });
    request(editor, address, "setVariable", arguments, |editor, address, result| match result {
        Ok(_) => load_frame(editor, address),
        Err(error) => editor.set_error(format!("Could not set it: {error}")),
    });
}
