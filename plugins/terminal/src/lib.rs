//! Built-in plugin: terminals running a shell, like Emacs `vterm`. Each terminal is a
//! buffer drawn as a screen; one at a time shows in the panel under the text, or any
//! takes the whole window. Keys go to the shell, but for remacs's own (⌘ chords and
//! `editor-keys`); copy mode reads the scrollback with Emacs keys.

mod commands;
mod copy;
mod grid;
mod history;
mod integration;
mod keys;
mod links;
mod mouse;
mod scan;
mod session;
mod settings;
mod terminal;
mod terminals;
mod view;

use std::rc::Rc;

use remacs_core::{Command, Editor, Keymap, MenuItem, MinorKeymap, PaneSource, Plugin, Registry};

use crate::settings::TerminalSettings;
use crate::terminals::{PANE, all, close, find_mut, is_shown, keyed, rename_buffers, terminals};
use crate::view::{CLOSE, LIST, MAXIMIZE, NEW, TerminalScreens};

pub use crate::terminal::find_program;
pub use crate::terminals::Terminals;

pub struct TerminalPlugin;

const TOGGLE: &str = "terminal";
const NEXT: &str = "terminal-next";
const PREVIOUS: &str = "terminal-previous";
const RENAME: &str = "terminal-rename";
const COPY_MODE: &str = "terminal-copy-mode";
const SEARCH: &str = "terminal-search";
const COPY: &str = "terminal-copy";
const PASTE: &str = "terminal-paste";
const CLEAR: &str = "terminal-clear";
const PREVIOUS_PROMPT: &str = "terminal-previous-prompt";
const NEXT_PROMPT: &str = "terminal-next-prompt";
const RESTART: &str = "terminal-restart";
const CLOSE_ENDED: &str = "terminal-close-ended";
const OPEN_SETTINGS: &str = "terminal-open-settings";
const START_FALLBACK: &str = "terminal-start-fallback";

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(TOGGLE, "Show a terminal where the settings open them, give it the keys, or hide it", commands::toggle),
    Command::new(NEW, "Start another terminal where the current one is", commands::new_terminal),
    Command::new(NEXT, "Show the next terminal", commands::next),
    Command::new(PREVIOUS, "Show the previous terminal", commands::previous),
    Command::new(MAXIMIZE, "Move the terminal from the panel to the window, or back", commands::maximize),
    Command::new(CLOSE, "Close the terminal, killing what runs in it", commands::close_current),
    Command::new(RENAME, "Name the terminal, in place of its title", commands::rename),
    Command::new(LIST, "Choose a terminal from all of them", commands::list),
    Command::new(COPY_MODE, "Read the terminal's scrollback with Emacs keys and copy from it", commands::copy_mode),
    Command::new(SEARCH, "Search the terminal's scrollback", commands::search),
    Command::new(COPY, "Copy the text selected in the terminal", commands::copy_selection),
    Command::new(PASTE, "Type the newest kill into the terminal", commands::paste_kill),
    Command::new(CLEAR, "Clear the terminal's screen and scrollback", commands::clear),
    Command::new(PREVIOUS_PROMPT, "Scroll the terminal to the previous command's prompt", commands::previous_prompt),
    Command::new(NEXT_PROMPT, "Scroll the terminal to the next command's prompt", commands::next_prompt),
    Command::new(RESTART, "Start the ended terminal's shell again", commands::restart),
    Command::new(CLOSE_ENDED, "Close the ended terminal", commands::close_ended),
    Command::new(OPEN_SETTINGS, "Visit terminal.toml at the shell it names", commands::open_settings),
    Command::new(START_FALLBACK, "Start zsh in the terminal whose shell could not start, this once", commands::start_fallback),
];

const BINDINGS: &[(&str, &str)] = &[
    ("M-<f12>", TOGGLE),
    ("C-`", TOGGLE),
    ("s-t", NEW),
    ("s-RET", MAXIMIZE),
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("View", "panels", "Terminal", TOGGLE),
    MenuItem::new("View", "panels", "New Terminal", NEW),
    MenuItem::new("View", "panels", "Maximize Terminal", MAXIMIZE),
];

/// While a terminal has the keys.
const TERMINAL_BINDINGS: &[(&str, &str)] = &[
    ("s-}", NEXT),
    ("s-{", PREVIOUS),
    ("C-x k", CLOSE),
    ("C-x r", RENAME),
    ("C-x C-b", LIST),
    ("C-x [", COPY_MODE),
    ("s-f", SEARCH),
    ("s-c", COPY),
    ("s-v", PASTE),
    ("s-k", CLEAR),
    ("s-<up>", PREVIOUS_PROMPT),
    ("s-<down>", NEXT_PROMPT),
];

/// While the terminal with the keys has ended.
const EXITED_BINDINGS: &[(&str, &str)] = &[("RET", RESTART), ("q", CLOSE_ENDED)];
const FAILED_BINDINGS: &[(&str, &str)] = &[
    ("RET", OPEN_SETTINGS),
    ("z", START_FALLBACK),
    ("r", RESTART),
];

impl Plugin for TerminalPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend_from_slice(MENU_ITEMS);
        registry.minor_keymaps.push(MinorKeymap {
            active: view::ended_has_keys,
            keymap: keymap(EXITED_BINDINGS),
        });
        registry.minor_keymaps.push(MinorKeymap {
            active: view::failed_has_keys,
            keymap: keymap(FAILED_BINDINGS),
        });
        registry.minor_keymaps.push(MinorKeymap {
            active: view::has_keys,
            keymap: keymap(TERMINAL_BINDINGS),
        });
        registry.typing_blockers.push(view::has_keys);
        registry.key_sinks.push(Rc::new(view::take_key));
        registry.screens.push(Rc::new(TerminalScreens));
        registry.panes.push(PaneSource {
            name: PANE,
            shown: view::pane_shown,
            describe: Rc::new(view::pane),
        });
        registry.text_controls.push(Rc::new(view::controls));
        registry.status_providers.push(Rc::new(view::status));
        registry.settings_hooks.push(Rc::new(read_settings));
        registry.post_command_hooks.push(Rc::new(update));
    }
}

fn keymap(bindings: &[(&'static str, &'static str)]) -> Keymap {
    let mut keymap = Keymap::default();
    for &(keys, command) in bindings {
        keymap.bind(keys, command);
    }
    keymap
}

fn read_settings(editor: &mut Editor) {
    let settings = editor.settings_section::<TerminalSettings>();
    let state = terminals(editor);
    state.kept_keys = settings.kept_keys();
    state.settings = settings;
}

/// Takes what the terminals' threads reported: output, titles, bells, exits.
fn update(editor: &mut Editor) {
    let close_on_exit = terminals(editor).settings.close_on_exit;
    let ids: Vec<u64> = all(editor).iter().map(|terminal| terminal.id).collect();
    let with_keys = keyed(editor);
    for id in ids {
        let shown = is_shown(editor, id);
        let Some(pumped) = find_mut(editor, id).map(|terminal| {
            terminal.report_focus(with_keys == Some(id));
            terminal.pump(shown)
        }) else {
            continue;
        };
        if let Some(text) = pumped.clipboard {
            editor.kill_ring.push(text);
        }
        if pumped.rang {
            // Once more when the flash is over.
            editor.refresh_after(view::FLASH);
        }
        if pumped.exited && close_on_exit {
            close(editor, id);
        }
    }
    rename_buffers(editor);
    let alive: Vec<u64> = all(editor).iter().map(|terminal| terminal.id).collect();
    let panel = &mut terminals(editor).panel;
    if panel.current.is_some_and(|id| !alive.contains(&id)) {
        panel.current = alive.first().copied();
        panel.open &= panel.current.is_some();
    }
}

#[cfg(test)]
mod tests;
