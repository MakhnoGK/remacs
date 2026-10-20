//! All terminals: each is a buffer; the panel under the text shows one at a time, or the
//! window shows one in full.

use std::path::PathBuf;

use remacs_core::{Buffer, Editor, Key, TabState};
use remacs_files::{default_directory, project_root};
use remacs_text::Text;

use crate::session::Phase;
use crate::settings::{Placement, StartDirectory, TerminalSettings};
use crate::terminal::{Terminal, configured_shell};

/// In the switch between panes.
pub const PANE: &str = "Terminal";

#[derive(Default)]
pub struct Terminals {
    next_id: u64,
    pub settings: TerminalSettings,
    /// The first keys of `editor-keys`.
    pub kept_keys: Vec<Key>,
    pub panel: Panel,
    /// Shown in the window before a terminal took it, to go back to.
    pub previous_buffer: Option<String>,
}

#[derive(Default)]
pub struct Panel {
    /// Wanted under the text; it shows while some terminal is not the window's.
    pub open: bool,
    /// The terminal it shows.
    pub current: Option<u64>,
}

pub fn terminals(editor: &mut Editor) -> &mut Terminals {
    editor.plugin_state::<Terminals>()
}

pub fn settings(editor: &Editor) -> TerminalSettings {
    editor
        .find_plugin_state::<Terminals>()
        .map(|terminals| terminals.settings.clone())
        .unwrap_or_default()
}

/// Every terminal, oldest first.
pub fn all(editor: &Editor) -> Vec<&Terminal> {
    let mut all: Vec<&Terminal> = editor
        .buffers()
        .iter()
        .filter_map(Buffer::local::<Terminal>)
        .collect();
    all.sort_by_key(|terminal| terminal.id);
    all
}

/// The buffers of terminals running more than their shell.
pub fn running(editor: &Editor) -> Vec<String> {
    editor
        .buffers()
        .iter()
        .filter(|buffer| {
            buffer
                .local::<Terminal>()
                .is_some_and(|terminal| terminal.tab_state() == TabState::Running)
        })
        .map(|buffer| buffer.name().to_owned())
        .collect()
}

pub fn find(editor: &Editor, id: u64) -> Option<&Terminal> {
    editor
        .buffers()
        .iter()
        .filter_map(Buffer::local::<Terminal>)
        .find(|terminal| terminal.id == id)
}

pub fn find_mut(editor: &mut Editor, id: u64) -> Option<&mut Terminal> {
    editor
        .buffers_mut()
        .iter_mut()
        .filter_map(Buffer::local_mut::<Terminal>)
        .find(|terminal| terminal.id == id)
}

/// The name of the buffer holding terminal `id`.
pub fn buffer_name(editor: &Editor, id: u64) -> Option<String> {
    editor
        .buffers()
        .iter()
        .find(|buffer| {
            buffer
                .local::<Terminal>()
                .is_some_and(|terminal| terminal.id == id)
        })
        .map(|buffer| buffer.name().to_owned())
}

/// The terminal shown in the window, if one is.
pub fn in_window(editor: &Editor) -> Option<u64> {
    editor
        .window_buffer()
        .local::<Terminal>()
        .map(|terminal| terminal.id)
}

/// The terminal in the panel, while the panel shows.
pub fn in_panel(editor: &Editor) -> Option<u64> {
    let state = editor.find_plugin_state::<Terminals>()?;
    let current = state.panel.current?;
    (state.panel.open && in_window(editor).is_none() && find(editor, current).is_some())
        .then_some(current)
}

/// The terminal the keys go to: the panel's while it has them, else the window's.
pub fn keyed(editor: &Editor) -> Option<u64> {
    if editor.pane_focused() {
        return editor
            .pane_has_keys(PANE)
            .then(|| in_panel(editor))
            .flatten();
    }
    if editor.minibuffer().is_some() {
        return None;
    }
    in_window(editor)
}

/// The terminal commands act on: the one with the keys, else the panel's.
pub fn current(editor: &Editor) -> Option<u64> {
    keyed(editor)
        .or_else(|| in_window(editor))
        .or_else(|| in_panel(editor))
}

/// Whether terminal `id` is on screen, so what it prints is seen.
pub fn is_shown(editor: &Editor, id: u64) -> bool {
    in_window(editor) == Some(id)
        || (in_panel(editor) == Some(id) && editor.shown_pane() == Some(PANE))
}

/// Starts a terminal where the settings place new ones, or where `placement` says.
pub fn create(editor: &mut Editor, placement: Placement) -> u64 {
    let settings = settings(editor);
    let directory = start_directory(editor, settings.directory);
    let state = terminals(editor);
    state.next_id += 1;
    let id = state.next_id;
    let mut terminal = Terminal::new(
        id,
        configured_shell(&settings),
        settings.args.clone(),
        directory,
    );
    terminal.start(&settings, editor.notifier());
    add(editor, terminal, placement);
    id
}

/// Starts terminal `terminal` in a buffer of its own and shows it.
pub fn add(editor: &mut Editor, terminal: Terminal, placement: Placement) {
    let id = terminal.id;
    let mut buffer = Buffer::new(
        terminal.buffer_name(),
        Text::default(),
        editor.mode_for(None),
    )
    .with_directory(terminal.directory.clone())
    .read_only();
    buffer.set_local(terminal);
    match placement {
        Placement::Panel => {
            editor.add_buffer_noselect(buffer);
            show_in_panel(editor, id);
        }
        Placement::Buffer => {
            remember_window(editor);
            editor.add_buffer(buffer);
            editor.focus_pane(false);
        }
    }
}

/// Shows terminal `id` in the panel, with the keys.
pub fn show_in_panel(editor: &mut Editor, id: u64) {
    if in_window(editor).is_some() {
        restore_window(editor);
    }
    let panel = &mut terminals(editor).panel;
    panel.open = true;
    panel.current = Some(id);
    editor.select_pane(PANE);
    editor.focus_pane(true);
}

/// Shows terminal `id` in the window.
pub fn show_in_window(editor: &mut Editor, id: u64) {
    let Some(name) = buffer_name(editor, id) else {
        return;
    };
    remember_window(editor);
    // Moved, not copied: leaving the window later does not bring it back under the text.
    terminals(editor).panel.open = false;
    editor.switch_to_buffer(&name);
    editor.focus_pane(false);
}

/// Shows terminal `id` where the current one is.
pub fn show_here(editor: &mut Editor, id: u64) {
    if in_window(editor).is_some() {
        show_in_window(editor, id);
    } else {
        show_in_panel(editor, id);
    }
}

fn remember_window(editor: &mut Editor) {
    if in_window(editor).is_none() {
        let name = editor.window_buffer().name().to_owned();
        terminals(editor).previous_buffer = Some(name);
    }
}

/// Gives the window back to the buffer it showed before a terminal, else to the most
/// recent buffer that is not a terminal.
pub fn restore_window(editor: &mut Editor) {
    let previous = terminals(editor).previous_buffer.take();
    let name = previous
        .filter(|name| {
            editor
                .buffers()
                .iter()
                .any(|buffer| buffer.name() == name && buffer.local::<Terminal>().is_none())
        })
        .or_else(|| {
            editor
                .buffers()
                .iter()
                .find(|buffer| buffer.local::<Terminal>().is_none())
                .map(|buffer| buffer.name().to_owned())
        });
    match name {
        Some(name) => {
            editor.switch_to_buffer(&name);
        }
        None => {
            let mode = editor.mode_for(None);
            editor.add_buffer(Buffer::new(
                remacs_core::SCRATCH_BUFFER,
                Text::default(),
                mode,
            ));
        }
    }
}

/// Closes terminal `id`, killing what runs in it; the panel moves to a neighbor.
pub fn close(editor: &mut Editor, id: u64) {
    let Some(name) = buffer_name(editor, id) else {
        return;
    };
    let ids: Vec<u64> = all(editor).iter().map(|terminal| terminal.id).collect();
    let neighbor = neighbor(&ids, id, 1).filter(|&other| other != id);
    let was_window = in_window(editor) == Some(id);
    let was_panel = terminals(editor).panel.current == Some(id);
    editor.kill_buffer(&name);
    if was_window {
        match neighbor {
            Some(other) => {
                if let Some(other_name) = buffer_name(editor, other) {
                    editor.switch_to_buffer(&other_name);
                }
            }
            None => restore_window(editor),
        }
    }
    if was_panel {
        let panel = &mut terminals(editor).panel;
        panel.current = neighbor;
        if neighbor.is_none() {
            panel.open = false;
            editor.focus_pane(false);
        }
    }
}

/// The id `step` places after `id` among `ids`, wrapping around.
pub fn neighbor(ids: &[u64], id: u64, step: isize) -> Option<u64> {
    let index = ids.iter().position(|&other| other == id)?;
    let count = ids.len() as isize;
    let next = (index as isize + step).rem_euclid(count) as usize;
    ids.get(next).copied()
}

fn start_directory(editor: &Editor, start: StartDirectory) -> PathBuf {
    match start {
        StartDirectory::Project => project_root(&default_directory(editor)),
        StartDirectory::File => default_directory(editor),
        StartDirectory::Home => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/")),
    }
}

/// Renames terminal buffers whose label changed.
pub fn rename_buffers(editor: &mut Editor) {
    let renames: Vec<(String, String)> = editor
        .buffers()
        .iter()
        .filter_map(|buffer| {
            let wanted = buffer.local::<Terminal>()?.buffer_name();
            (without_number(buffer.name()) != wanted).then(|| (buffer.name().to_owned(), wanted))
        })
        .collect();
    for (name, wanted) in renames {
        editor.rename_buffer(&name, &wanted);
    }
}

/// `name<2>` as `name`, as a taken name was made unique.
fn without_number(name: &str) -> &str {
    name.strip_suffix('>')
        .and_then(|rest| rest.rsplit_once('<'))
        .filter(|(_, number)| number.parse::<usize>().is_ok())
        .map_or(name, |(base, _)| base)
}

pub fn is_ended(terminal: &Terminal) -> bool {
    matches!(terminal.phase, Phase::Exited(_) | Phase::Failed(_))
}
