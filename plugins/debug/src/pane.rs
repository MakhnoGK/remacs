//! The debug pane under the text: the session's toolbar, then its threads and frames, the
//! selected frame's variables and the watches, each a column the keys can move to. Also
//! the paused line's band and the values of the variables on the lines above it.

use std::ops::Range;
use std::rc::Rc;

use remacs_core::{
    Command, CommandResult, Completions, Editor, LineExtra, Pane, PaneBody, PaneButton, PaneColumn,
    PaneRow, Prompt, Tone,
};

use crate::edit::location;
use crate::inspect::{
    COLUMNS, Column, Entry, Inspected, View, evaluate_watches, select_frame, set_value,
    toggle_open, watch_path,
};
use crate::session::{Address, Debugger, Link, Session, debugger, find_link};

pub const FOCUS_FRAMES: &str = "debug-focus-frames";
pub const FOCUS_VARIABLES: &str = "debug-focus-variables";
pub const FOCUS_WATCHES: &str = "debug-focus-watches";
pub const ADD_WATCH: &str = "debug-add-watch";
const NEXT: &str = "debug-pane-next";
const PREVIOUS: &str = "debug-pane-previous";
const TOGGLE: &str = "debug-pane-toggle";
const ACTIVATE: &str = "debug-pane-activate";
const EDIT: &str = "debug-pane-edit";
const DELETE: &str = "debug-pane-delete";
const LEAVE: &str = "debug-pane-leave";

/// In the switch between panes.
pub const NAME: &str = "Debug";

#[rustfmt::skip]
pub const COMMANDS: &[Command] = &[
    Command::new(FOCUS_FRAMES, "Move the keys to the debug pane's threads and frames", focus_frames),
    Command::new(FOCUS_VARIABLES, "Move the keys to the debug pane's variables", focus_variables),
    Command::new(FOCUS_WATCHES, "Move the keys to the debug pane's watches", focus_watches),
    Command::new(ADD_WATCH, "Watch an expression, evaluated at every stop", add_watch),
    Command::new(NEXT, "Select the next row of the debug pane", next),
    Command::new(PREVIOUS, "Select the previous row of the debug pane", previous),
    Command::new(TOGGLE, "Open the selected variable, or close it", toggle),
    Command::new(ACTIVATE, "Show the selected frame, open the selected variable or edit the selected watch", activate),
    Command::new(EDIT, "Set the selected variable's value, or edit the selected watch", edit),
    Command::new(DELETE, "Stop watching the selected expression", delete),
    Command::new(LEAVE, "Give the keys back to the text", leave),
];

/// While a column has the keys.
pub const FOCUSED_BINDINGS: &[(&str, &str)] = &[
    ("C-n", NEXT),
    ("<down>", NEXT),
    ("C-p", PREVIOUS),
    ("<up>", PREVIOUS),
    ("TAB", TOGGLE),
    ("RET", ACTIVATE),
    ("e", EDIT),
    ("d", DELETE),
    ("DEL", DELETE),
    ("a", ADD_WATCH),
    ("C-g", LEAVE),
    ("ESC", LEAVE),
];

const PAUSED_HINTS: &[(&str, &str)] = &[
    ("F8", "Step over"),
    ("F7", "Step into"),
    ("F9", "Resume"),
    ("C-x o", "Pane"),
];
const RUNNING_HINTS: &[(&str, &str)] =
    &[("⌥F9", "Pause"), ("⌘F8", "Breakpoint"), ("C-x o", "Pane")];
const FRAMES_HINTS: &[(&str, &str)] = &[("↵", "Show")];
const VARIABLES_HINTS: &[(&str, &str)] = &[("TAB", "Expand"), ("e", "Set value")];
const WATCHES_HINTS: &[(&str, &str)] = &[("a", "Add"), ("e", "Edit"), ("d", "Delete")];

/// What a row stands for, for the keys and clicks that act on it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Nothing,
    Frame(usize),
    Opens {
        path: String,
        reference: i64,
    },
    Variable {
        path: String,
        reference: i64,
        container: i64,
        name: String,
        value: String,
    },
    Watch(usize),
    AddWatch,
}

/// The session the pane shows, and its connection that stopped last.
pub fn shown(editor: &Editor) -> Option<(&Session, Address)> {
    let debugger = editor.find_plugin_state::<Debugger>()?;
    let debugging = |id: u64| {
        debugger
            .sessions
            .iter()
            .find(|session| session.id == id && session.is_debugging())
    };
    if let Some(address) = debugger.current
        && let Some(session) = debugging(address.session)
    {
        return Some((session, address));
    }
    let session = debugger
        .sessions
        .iter()
        .rev()
        .find(|session| session.is_debugging())?;
    let address = Address {
        session: session.id,
        link: session.links.len() - 1,
    };
    Some((session, address))
}

pub fn is_shown(editor: &Editor) -> bool {
    shown(editor).is_some()
}

fn shown_link(editor: &Editor) -> Option<(&Session, &Link, Address)> {
    let (session, address) = shown(editor)?;
    Some((session, session.link(address.link)?, address))
}

pub fn pane(editor: &Editor) -> Option<Pane> {
    let (session, link, _) = shown_link(editor)?;
    let debugger = editor.find_plugin_state::<Debugger>()?;
    let view = &debugger.view;
    let paused = link.stopped.is_some();
    let state = match &link.stopped {
        Some(stop) => (format!("Paused · {}", stop.reason), Tone::Warning),
        None => ("Running".to_owned(), Tone::Success),
    };
    let button = |label, keys, enabled, command: &'static str| PaneButton {
        label,
        keys,
        enabled,
        action: Rc::new(move |editor: &mut Editor| {
            let run = editor.commands().get(command).ok_or("No such command")?.run;
            run(editor)
        }),
    };
    let mut buttons = vec![if paused {
        button("Resume", "F9", true, "debug-continue")
    } else {
        button("Pause", "⌥F9", true, "debug-pause")
    }];
    buttons.extend([
        button("Step over", "F8", paused, "debug-step-over"),
        button("Step into", "F7", paused, "debug-step-into"),
        button("Step out", "⇧F8", paused, "debug-step-out"),
        button("Stop", "⌘F2", true, "run-stop"),
    ]);
    let columns = COLUMNS
        .iter()
        .map(|&column| {
            let rows = rows(column, link, view);
            let focused = has_keys(editor) && view.focus == column;
            let selected = view.selected[column.index()].min(rows.len().saturating_sub(1));
            let (title, keys, width, empty, hints) = match column {
                Column::Frames => (
                    "Threads & frames",
                    "C-c f",
                    Some(400.0),
                    "No call stack while the program runs. Pause or hit a breakpoint to inspect frames.",
                    FRAMES_HINTS,
                ),
                Column::Variables => (
                    "Variables",
                    "C-c v",
                    None,
                    "Variables appear when execution stops.",
                    VARIABLES_HINTS,
                ),
                Column::Watches => ("Watches", "C-c w", Some(380.0), "", WATCHES_HINTS),
            };
            PaneColumn {
                title,
                keys,
                width,
                rows: rows
                    .into_iter()
                    .enumerate()
                    .map(|(index, (row, _))| PaneRow {
                        action: Some(Rc::new(move |editor: &mut Editor| {
                            select(editor, column, index);
                            activate(editor)
                        })),
                        ..row
                    })
                    .collect(),
                selected: focused.then_some(selected),
                focused,
                empty: empty.to_owned(),
                hints,
            }
        })
        .collect();
    let subtitle = session.adapter().to_owned();
    Some(Pane {
        count: None,
        height: debugger.settings.pane_height,
        title: session.name.clone(),
        subtitle,
        state,
        tabs: None,
        buttons,
        body: PaneBody::Columns(columns),
        hints: if paused { PAUSED_HINTS } else { RUNNING_HINTS },
    })
}

fn rows(column: Column, link: &Link, view: &View) -> Vec<(PaneRow, Target)> {
    match column {
        Column::Frames => frame_rows(link),
        Column::Variables => variable_rows(&link.inspected, view),
        Column::Watches => watch_rows(link, view),
    }
}

fn frame_rows(link: &Link) -> Vec<(PaneRow, Target)> {
    let Some(stop) = &link.stopped else {
        return Vec::new();
    };
    let mut threads = link.inspected.threads.clone();
    if !threads.iter().any(|(id, _)| *id == stop.thread) {
        threads.insert(0, (stop.thread, format!("Thread {}", stop.thread)));
    }
    let mut rows = Vec::new();
    for (id, name) in threads {
        let stopped = id == stop.thread;
        rows.push((
            PaneRow {
                open: Some(stopped),
                name,
                tag: Some(if stopped {
                    (stop.reason.clone(), Tone::Warning)
                } else {
                    ("paused".to_owned(), Tone::Faint)
                }),
                ..PaneRow::default()
            },
            Target::Nothing,
        ));
        if !stopped {
            continue;
        }
        for (index, frame) in link.frames.iter().enumerate() {
            let detail = frame
                .path
                .as_deref()
                .map(|path| location(path, frame.line.saturating_sub(1)))
                .unwrap_or_default();
            rows.push((
                PaneRow {
                    depth: 1,
                    name: frame.name.clone(),
                    detail,
                    tone: if frame.subtle {
                        Tone::Faint
                    } else {
                        Tone::Plain
                    },
                    current: index == link.frame,
                    ..PaneRow::default()
                },
                Target::Frame(index),
            ));
        }
    }
    rows
}

fn changed_tag(view: &View, path: &str, value: &str) -> Option<(String, Tone)> {
    view.previous
        .get(path)
        .is_some_and(|previous| previous != value)
        .then(|| ("changed".to_owned(), Tone::Warning))
}

fn variable_rows(inspected: &Inspected, view: &View) -> Vec<(PaneRow, Target)> {
    entry_rows(inspected.entries(&view.open), view)
}

fn entry_rows(entries: Vec<Entry>, view: &View) -> Vec<(PaneRow, Target)> {
    entries
        .into_iter()
        .map(|entry| match entry {
            Entry::Scope {
                name,
                path,
                open,
                reference,
            } => (
                PaneRow {
                    open: Some(open),
                    name: name.to_owned(),
                    tone: Tone::Faint,
                    ..PaneRow::default()
                },
                Target::Opens { path, reference },
            ),
            Entry::Variable {
                variable,
                path,
                depth,
                open,
                container,
            } => (
                PaneRow {
                    depth,
                    open: (variable.reference > 0).then_some(open),
                    name: variable.name.clone(),
                    kind: variable.kind.clone(),
                    value: Some(variable.value.clone()),
                    tag: changed_tag(view, &path, &variable.value),
                    ..PaneRow::default()
                },
                Target::Variable {
                    path,
                    reference: variable.reference,
                    container,
                    name: variable.name.clone(),
                    value: variable.value.clone(),
                },
            ),
        })
        .collect()
}

fn watch_rows(link: &Link, view: &View) -> Vec<(PaneRow, Target)> {
    let inspected = &link.inspected;
    let mut rows = Vec::new();
    for (index, expression) in view.watches.iter().enumerate() {
        let result = inspected.watches.get(index).cloned().flatten();
        let path = watch_path(expression);
        let (value, kind, reference) = match &result {
            Some(Ok(watch)) => (watch.value.clone(), watch.kind.clone(), watch.reference),
            Some(Err(error)) => (first_error(error), String::new(), 0),
            None => ("— not available".to_owned(), String::new(), 0),
        };
        let open = reference > 0 && view.open.contains(&path);
        rows.push((
            PaneRow {
                open: (reference > 0).then_some(open),
                name: expression.clone(),
                kind,
                tag: changed_tag(view, &path, &value),
                value: Some(value),
                tone: if result.is_some() {
                    Tone::Plain
                } else {
                    Tone::Faint
                },
                ..PaneRow::default()
            },
            Target::Watch(index),
        ));
        if open {
            rows.extend(entry_rows(
                inspected.entries_under(reference, &path, 1, &view.open),
                view,
            ));
        }
    }
    rows.push((
        PaneRow {
            name: "+ Add expression…".to_owned(),
            tone: Tone::Faint,
            ..PaneRow::default()
        },
        Target::AddWatch,
    ));
    rows
}

/// The column with the keys, and the target of its selected row.
fn selected_target(editor: &Editor) -> Option<(Column, Target, Address)> {
    let (_, link, address) = shown_link(editor)?;
    let view = &editor.find_plugin_state::<Debugger>()?.view;
    let column = has_keys(editor).then_some(view.focus)?;
    let rows = rows(column, link, view);
    let index = view.selected[column.index()].min(rows.len().checked_sub(1)?);
    Some((column, rows.into_iter().nth(index)?.1, address))
}

fn select(editor: &mut Editor, column: Column, index: usize) {
    debugger(editor).view.selected[column.index()] = index;
    give_keys(editor, column);
}

fn focus(editor: &mut Editor, column: Column) -> CommandResult {
    shown(editor).ok_or("Nothing is being debugged")?;
    give_keys(editor, column);
    Ok(())
}

fn give_keys(editor: &mut Editor, column: Column) {
    debugger(editor).view.focus = column;
    editor.select_pane(NAME);
    editor.focus_pane(true);
}

fn focus_frames(editor: &mut Editor) -> CommandResult {
    focus(editor, Column::Frames)
}

fn focus_variables(editor: &mut Editor) -> CommandResult {
    focus(editor, Column::Variables)
}

fn focus_watches(editor: &mut Editor) -> CommandResult {
    focus(editor, Column::Watches)
}

fn leave(editor: &mut Editor) -> CommandResult {
    editor.focus_pane(false);
    Ok(())
}

/// The pane is shown and has the keys, unless a prompt has them.
pub fn has_keys(editor: &Editor) -> bool {
    editor.pane_has_keys(NAME)
}

fn step(editor: &mut Editor, forward: bool) -> CommandResult {
    let (_, link, _) = shown_link(editor).ok_or("Nothing is being debugged")?;
    let view = &editor
        .find_plugin_state::<Debugger>()
        .ok_or("No pane")?
        .view;
    let column = view.focus;
    let count = rows(column, link, view).len();
    let selected = &mut debugger(editor).view.selected[column.index()];
    *selected = if forward {
        (*selected + 1).min(count.saturating_sub(1))
    } else {
        selected.saturating_sub(1).min(count.saturating_sub(1))
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
    let (_, target, address) = selected_target(editor).ok_or("Nothing selected")?;
    match target {
        Target::Opens { path, reference }
        | Target::Variable {
            path, reference, ..
        } if reference > 0 => {
            toggle_open(editor, address, path, reference);
        }
        Target::Watch(index) => {
            let reference = watch_reference(editor, address, index).ok_or("Nothing to open")?;
            let path = watch_path(&debugger(editor).view.watches[index]);
            toggle_open(editor, address, path, reference);
        }
        _ => return Err("Nothing to open".into()),
    }
    Ok(())
}

fn watch_reference(editor: &mut Editor, address: Address, index: usize) -> Option<i64> {
    let link = find_link(editor, address)?;
    match link.inspected.watches.get(index)? {
        Some(Ok(watch)) if watch.reference > 0 => Some(watch.reference),
        _ => None,
    }
}

fn activate(editor: &mut Editor) -> CommandResult {
    let (_, target, address) = selected_target(editor).ok_or("Nothing selected")?;
    match target {
        Target::Frame(index) => {
            select_frame(editor, address, index);
            Ok(())
        }
        Target::Opens { .. } | Target::Variable { .. } => toggle(editor),
        Target::Watch(index) => edit_watch(editor, Some(index)),
        Target::AddWatch => edit_watch(editor, None),
        Target::Nothing => Ok(()),
    }
}

fn edit(editor: &mut Editor) -> CommandResult {
    let (_, target, address) = selected_target(editor).ok_or("Nothing selected")?;
    match target {
        Target::Variable {
            container,
            name,
            value,
            ..
        } => {
            let prompt = Prompt {
                label: format!("Set {name}"),
                initial_input: value,
                ..Prompt::default()
            };
            editor.read(
                prompt,
                |_, _| Completions::default(),
                move |editor, value| {
                    set_value(editor, address, container, name, value);
                    Ok(())
                },
            )
        }
        Target::Watch(index) => edit_watch(editor, Some(index)),
        Target::AddWatch => edit_watch(editor, None),
        _ => Err("Nothing to edit".into()),
    }
}

fn delete(editor: &mut Editor) -> CommandResult {
    let (_, target, address) = selected_target(editor).ok_or("Nothing selected")?;
    let Target::Watch(index) = target else {
        return Err("Only watches are deleted".into());
    };
    debugger(editor).view.watches.remove(index);
    evaluate_watches(editor, address);
    Ok(())
}

fn add_watch(editor: &mut Editor) -> CommandResult {
    edit_watch(editor, None)
}

/// Asks for the expression of watch `index`, or of a new one.
fn edit_watch(editor: &mut Editor, index: Option<usize>) -> CommandResult {
    let initial_input = index
        .and_then(|index| debugger(editor).view.watches.get(index).cloned())
        .unwrap_or_default();
    let prompt = Prompt {
        label: "Watch".into(),
        placeholder: "An expression, evaluated at every stop; empty removes it",
        initial_input,
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        move |editor, expression| {
            let expression = expression.trim().to_owned();
            let watches = &mut debugger(editor).view.watches;
            match (index, expression.is_empty()) {
                (Some(index), true) => {
                    watches.remove(index);
                }
                (Some(index), false) => watches[index] = expression,
                (None, true) => {}
                (None, false) => watches.push(expression),
            }
            if let Some((_, address)) = shown(editor) {
                evaluate_watches(editor, address);
            }
            Ok(())
        },
    )
}

/// The paused line's band and, on the lines above it, the values of the variables they
/// declare.
pub fn line_extras(editor: &Editor, lines: Range<usize>) -> Vec<LineExtra> {
    let Some((_, link, _)) = shown_link(editor) else {
        return Vec::new();
    };
    let buffer = editor.window_buffer();
    let Some(frame) = link.frames.get(link.frame) else {
        return Vec::new();
    };
    if link.stopped.is_none() || frame.path.as_deref() != buffer.path() {
        return Vec::new();
    }
    let paused = frame.line.saturating_sub(1);
    let settings = &editor
        .find_plugin_state::<Debugger>()
        .expect("shown")
        .settings;
    let mut extras: Vec<LineExtra> = Vec::new();
    if lines.contains(&paused) {
        extras.push(LineExtra {
            line: paused,
            band: Some(Tone::Warning),
            note: None,
        });
    }
    let first = paused.saturating_sub(settings.inline_value_lines);
    if settings.inline_value_lines == 0 || first > lines.end || paused < lines.start {
        return extras;
    }
    let text = |line: usize| {
        let range = buffer.text().line_range(line)?;
        Some(buffer.contents(range))
    };
    let mut notes: Vec<(usize, Vec<String>)> = Vec::new();
    for variable in link.inspected.locals() {
        let declared = (first..=paused)
            .rev()
            .find(|&line| text(line).is_some_and(|text| declares(&text, &variable.name)));
        let Some(line) = declared.filter(|line| lines.contains(line)) else {
            continue;
        };
        let value = shorten(&variable.value, settings.inline_value_length);
        let note = format!("{} = {value}", variable.name);
        match notes.iter_mut().find(|(at, _)| *at == line) {
            Some((_, shown)) => shown.push(note),
            None => notes.push((line, vec![note])),
        }
    }
    for (line, shown) in notes {
        let note = Some(shown.join(", "));
        match extras.iter_mut().find(|extra| extra.line == line) {
            Some(extra) => extra.note = note,
            None => extras.push(LineExtra {
                line,
                band: None,
                note,
            }),
        }
    }
    extras
}

/// Whether `line` declares `name`: binds it with `let`, `const`, `var` or `for`, or has
/// it among a function's parameters.
fn declares(line: &str, name: &str) -> bool {
    let words: Vec<&str> = line
        .split(|character: char| {
            !(character.is_alphanumeric() || character == '_' || character == '$')
        })
        .filter(|word| !word.is_empty())
        .collect();
    let Some(position) = words.iter().position(|word| *word == name) else {
        return false;
    };
    let before = &words[..position];
    let binder = ["let", "const", "var", "for", "mut"];
    let signature = words
        .iter()
        .any(|word| *word == "fn" || *word == "function")
        || line.contains("=>");
    signature || before.last().is_some_and(|word| binder.contains(word))
}

/// The line of an evaluation's error that says what went wrong, e.g. LLDB's `error:` one
/// after its notes.
fn first_error(message: &str) -> String {
    let lines = || {
        message
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
    };
    lines()
        .find_map(|line| {
            line.split_once("error: ")
                .map(|(_, error)| error.to_owned())
        })
        .or_else(|| lines().next().map(str::to_owned))
        .unwrap_or_default()
}

fn shorten(value: &str, length: usize) -> String {
    let flat = value.replace('\n', " ");
    if flat.chars().count() <= length {
        return flat;
    }
    let cut: String = flat.chars().take(length.saturating_sub(1)).collect();
    format!("{cut}…")
}
