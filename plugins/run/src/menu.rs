//! What can be done with an entry point, in a popup beside its ▶: run it, or edit its
//! configuration.

use std::mem;
use std::path::PathBuf;

use remacs_core::{Candidate, CommandResult, Editor, Popup, Selection};

use crate::commands::{edit_configuration, run_entry_point};
use crate::entry_points::EntryPoint;
use crate::launch::launch;
use crate::sessions::sessions;
use crate::window::{ENTRY_POINT_MENU, entry_point_for};

pub const NEXT: &str = "run-menu-next";
pub const PREVIOUS: &str = "run-menu-previous";
pub const CHOOSE: &str = "run-menu-choose";
pub const QUIT: &str = "run-menu-quit";
/// Any other command closes the popup.
const OWN_COMMANDS: &[&str] = &[ENTRY_POINT_MENU, NEXT, PREVIOUS, CHOOSE, QUIT];
const HINTS: &[(&str, &str)] = &[("↵", "Choose"), ("C-n C-p", "Next / previous")];

/// The debugger's command, offered when a plugin has it.
pub const DEBUG_AT_POINT: &str = "debug-at-point";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Item {
    Run,
    Debug,
    Edit,
}

fn items(editor: &Editor) -> Vec<Item> {
    let debug = editor.commands().get(DEBUG_AT_POINT).is_some();
    [
        Some(Item::Run),
        debug.then_some(Item::Debug),
        Some(Item::Edit),
    ]
    .into_iter()
    .flatten()
    .collect()
}

struct Menu {
    root: PathBuf,
    entry: EntryPoint,
    /// Its configuration's name, if the project has one already.
    saved: Option<String>,
    command_line: Option<String>,
    selection: Selection,
    /// Point when it opened; moving it closes the popup.
    point: usize,
}

#[derive(Default)]
pub struct State {
    menu: Option<Menu>,
    /// `Editor::command_count` when last looked at.
    seen_commands: u64,
}

fn state(editor: &mut Editor) -> &mut State {
    editor.plugin_state()
}

pub fn update(editor: &mut Editor) {
    let count = editor.command_count();
    if mem::replace(&mut state(editor).seen_commands, count) == count {
        return;
    }
    if !editor
        .last_command()
        .is_some_and(|command| OWN_COMMANDS.contains(&command))
    {
        state(editor).menu = None;
    }
}

/// Opens the menu of the entry point on point's line.
pub fn open(editor: &mut Editor) -> CommandResult {
    let line = editor.window_buffer().point_line();
    let (root, entry) = entry_point_for(editor, line)
        .filter(|(_, entry)| entry.line == line)
        .ok_or("No entry point here")?;
    let (project, error) = crate::configuration_store(editor).project(&root);
    let saved = project
        .find_kind(&entry.configuration.kind)
        .map(|configuration| configuration.name.clone());
    if let Some(error) = error {
        editor.set_error(error);
    }
    let settings = sessions(editor).settings.clone();
    let command_line = launch(&entry.configuration, &root, &settings)
        .ok()
        .map(|launch| launch.command_line());
    let point = editor.window_buffer().point();
    state(editor).menu = Some(Menu {
        root,
        entry,
        saved,
        command_line,
        selection: Selection::default(),
        point,
    });
    Ok(())
}

/// Point moved by the mouse hides the popup.
fn shown_menu(editor: &Editor) -> Option<&Menu> {
    if editor.minibuffer().is_some() {
        return None;
    }
    let point = editor.window_buffer().point();
    editor
        .find_plugin_state::<State>()?
        .menu
        .as_ref()
        .filter(|menu| menu.point == point)
}

pub fn is_shown(editor: &Editor) -> bool {
    shown_menu(editor).is_some()
}

pub fn popup(editor: &Editor) -> Option<Popup> {
    let menu = shown_menu(editor)?;
    let name = menu
        .saved
        .as_deref()
        .unwrap_or(&menu.entry.configuration.name);
    let unsaved = menu.saved.is_none();
    let temporary = unsaved && menu.entry.temporary;
    let rows = items(editor)
        .into_iter()
        .map(|item| match item {
            Item::Run => Candidate {
                detail: menu.command_line.clone().map(Into::into),
                tag: (unsaved && !temporary).then_some("new"),
                ..Candidate::new(format!("Run '{name}'"))
            },
            Item::Debug => Candidate::new(format!("Debug '{name}'")),
            Item::Edit if temporary => Candidate::new("Save configuration…"),
            Item::Edit => Candidate::new("Edit configuration…"),
        })
        .collect();
    let anchor = editor
        .window_buffer()
        .text()
        .line_start(menu.entry.line)
        .unwrap_or(menu.point);
    Some(Popup {
        anchor,
        rows,
        selected: Some(menu.selection.selected()),
        hints: HINTS,
        ..Popup::default()
    })
}

pub fn next(editor: &mut Editor) -> CommandResult {
    step(editor, true)
}

pub fn previous(editor: &mut Editor) -> CommandResult {
    step(editor, false)
}

fn step(editor: &mut Editor, forward: bool) -> CommandResult {
    let count = items(editor).len();
    let menu = state(editor).menu.as_mut().ok_or("No menu")?;
    let selected = menu.selection.selected();
    let index = if forward {
        selected + 1
    } else {
        selected.saturating_sub(1)
    };
    menu.selection.select(index, count, count);
    Ok(())
}

pub fn quit(editor: &mut Editor) -> CommandResult {
    state(editor).menu = None;
    Ok(())
}

pub fn choose(editor: &mut Editor) -> CommandResult {
    let items = items(editor);
    let menu = state(editor).menu.take().ok_or("No menu")?;
    match items[menu.selection.selected()] {
        Item::Run => run_entry_point(editor, &menu.root, menu.entry, None),
        Item::Debug => {
            let debug = editor
                .commands()
                .get(DEBUG_AT_POINT)
                .ok_or("No debugger")?
                .run;
            debug(editor)
        }
        Item::Edit => edit_configuration(editor, &menu.root, menu.entry.configuration),
    }
}
