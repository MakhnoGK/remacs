//! Built-in plugin: debugging run configurations through the Debug Adapter Protocol, like
//! IntelliJ's Debug. A configuration debugged runs in its console as when run; Cargo ones
//! are built first and debugged with codelldb, Node ones with js-debug, both downloaded
//! the first time they are needed.

mod adapter;
mod breakpoints;
mod dap;
mod edit;
mod evaluate;
mod inspect;
mod list;
mod marks;
mod pane;
mod prepare;
mod program;
mod session;
mod settings;

use std::path::Path;
use std::rc::Rc;

use remacs_config::state_dir;
use remacs_core::{
    Command, CommandResult, Editor, Keymap, MenuItem, MinorKeymap, PaneSource, Plugin, Registry,
    visiting_file,
};
use remacs_run::{DEBUG_AT_POINT, Starter};

use crate::marks::TOGGLE_BREAKPOINT;
use crate::session::{Address, Debugger, current, current_frame, debugger};
use crate::settings::DebugSettings;

pub struct DebugPlugin;

const DEBUG: &str = "debug";
const CONTINUE: &str = "debug-continue";
const STEP_OVER: &str = "debug-step-over";
const STEP_INTO: &str = "debug-step-into";
const STEP_OUT: &str = "debug-step-out";
const PAUSE: &str = "debug-pause";

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(DEBUG, "Debug the project's chosen configuration, or the console's again", debug),
    Command::new(DEBUG_AT_POINT, "Debug the entry point at point, saving a configuration for it the first time", debug_at_point),
    Command::new(CONTINUE, "Go on from where the program is paused", continue_program),
    Command::new(STEP_OVER, "Run the paused program to the next line", step_over),
    Command::new(STEP_INTO, "Step into the function called on the paused line", step_into),
    Command::new(STEP_OUT, "Run the paused program until its function returns", step_out),
    Command::new(PAUSE, "Pause the program being debugged", pause),
    Command::new(TOGGLE_BREAKPOINT, "Add a breakpoint on point's line, or remove the one there", marks::toggle_breakpoint),
];

/// IntelliJ's F-keys, which Emacs leaves free (ADR 0079): ⇧F9, ⌃⇧F9, F9, F8, F7, ⇧F8,
/// ⌥F9, ⌘F8, ⌘⇧F8, ⌥F8; the pane's columns under gud's `C-x C-a`. `keys.toml` binds
/// them otherwise.
const BINDINGS: &[(&str, &str)] = &[
    ("S-<f9>", DEBUG),
    ("C-S-<f9>", DEBUG_AT_POINT),
    ("<f9>", CONTINUE),
    ("<f8>", STEP_OVER),
    ("<f7>", STEP_INTO),
    ("S-<f8>", STEP_OUT),
    ("M-<f9>", PAUSE),
    ("s-<f8>", TOGGLE_BREAKPOINT),
    ("s-S-<f8>", edit::EDIT),
    ("C-x C-a f", pane::FOCUS_FRAMES),
    ("C-x C-a v", pane::FOCUS_VARIABLES),
    ("C-x C-a w", pane::FOCUS_WATCHES),
    ("M-<f8>", evaluate::EVALUATE),
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("Run", "debug", "Debug", DEBUG),
    MenuItem::new("Run", "debug", "Debug at Point", DEBUG_AT_POINT).enabled_when(visiting_file),
    MenuItem::new("Run", "step", "Continue", CONTINUE).enabled_when(stopped),
    MenuItem::new("Run", "step", "Pause", PAUSE).enabled_when(debugging),
    MenuItem::new("Run", "step", "Step Over", STEP_OVER).enabled_when(stopped),
    MenuItem::new("Run", "step", "Step Into", STEP_INTO).enabled_when(stopped),
    MenuItem::new("Run", "step", "Step Out", STEP_OUT).enabled_when(stopped),
    MenuItem::new("Run", "step", "Evaluate…", evaluate::EVALUATE).enabled_when(stopped),
    MenuItem::new("Run", "breakpoints", "Toggle Breakpoint", TOGGLE_BREAKPOINT).enabled_when(visiting_file),
    MenuItem::new("Run", "breakpoints", "Breakpoints", list::OPEN),
];

fn debugging(editor: &Editor) -> bool {
    editor
        .find_plugin_state::<Debugger>()
        .is_some_and(|debugger| !debugger.sessions.is_empty())
}

fn stopped(editor: &Editor) -> bool {
    current_frame(editor).is_some()
}

impl Plugin for DebugPlugin {
    fn register(&self, registry: &mut Registry) {
        let commands = COMMANDS
            .iter()
            .chain(edit::COMMANDS)
            .chain(list::COMMANDS)
            .chain(pane::COMMANDS)
            .chain(evaluate::COMMANDS);
        for command in commands {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend_from_slice(MENU_ITEMS);
        let mut focused = Keymap::default();
        for &(keys, command) in pane::FOCUSED_BINDINGS {
            focused.bind(keys, command);
        }
        registry.minor_keymaps.push(MinorKeymap {
            active: pane::has_keys,
            keymap: focused,
        });
        registry.typing_blockers.push(pane::has_keys);
        registry.settings_hooks.push(Rc::new(read_settings));
        registry.post_command_hooks.push(Rc::new(session::update));
        registry
            .post_command_hooks
            .push(Rc::new(marks::follow_edits));
        registry.gutters.push(Rc::new(marks::gutter_marks));
        registry.gutter_click.get_or_insert(TOGGLE_BREAKPOINT);
        registry.panes.push(PaneSource {
            name: pane::NAME,
            shown: pane::is_shown,
            describe: Rc::new(pane::pane),
        });
        registry.line_extras.push(Rc::new(pane::line_extras));
    }
}

fn read_settings(editor: &mut Editor) {
    let settings = editor.settings_section::<DebugSettings>();
    debugger(editor).settings = settings;
}

fn starter() -> Starter {
    Rc::new(session::start)
}

fn debug(editor: &mut Editor) -> CommandResult {
    remacs_run::run_with(editor, Some(starter()))
}

fn debug_at_point(editor: &mut Editor) -> CommandResult {
    remacs_run::run_at_point_with(editor, Some(starter()))
}

fn paused(editor: &mut Editor) -> Result<Address, &'static str> {
    current(editor).ok_or("Nothing is being debugged")
}

fn resume(editor: &mut Editor, command: &str) -> CommandResult {
    let address = paused(editor)?;
    session::resume(editor, address, command)?;
    Ok(())
}

fn continue_program(editor: &mut Editor) -> CommandResult {
    resume(editor, "continue")
}

fn step_over(editor: &mut Editor) -> CommandResult {
    resume(editor, "next")
}

fn step_into(editor: &mut Editor) -> CommandResult {
    resume(editor, "stepIn")
}

fn step_out(editor: &mut Editor) -> CommandResult {
    resume(editor, "stepOut")
}

fn pause(editor: &mut Editor) -> CommandResult {
    let address = paused(editor)?;
    session::pause(editor, address);
    Ok(())
}

/// Keeps breakpoints in `home`'s state directory across sessions.
pub fn keep_breakpoints_in(editor: &mut Editor, home: &Path) {
    let store = state_dir(home).join("breakpoints.json");
    debugger(editor).breakpoints.keep_in(store);
}

#[cfg(test)]
mod tests;
