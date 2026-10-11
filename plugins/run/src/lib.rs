//! Built-in plugin: running programs, like IntelliJ's run configurations. A project's
//! configurations live in its run file under `~/.config/remacs/run/`; running an entry
//! point (a ▶ in the gutter) for the first time adds one. Each configuration runs in its
//! own console buffer, its output colored and its `file:line`s links.

mod ansi;
mod chooser;
mod commands;
mod configuration;
mod console;
mod discovery;
mod entry_points;
mod launch;
mod menu;
mod process;
mod rust_tests;
mod sessions;
mod settings;
mod status;
mod window;

use std::path::Path;
use std::rc::Rc;

use remacs_config::{run_dir, state_dir};
use remacs_core::{
    Command, Editor, Keymap, MenuItem, MinorKeymap, Plugin, Registry, visiting_file,
};

use crate::configuration::Store;
use crate::console::Console;
use crate::settings::RunSettings;

pub use chooser::choose_with;
pub use commands::{run_at_point_with, run_with};
pub use configuration::{Cargo, Configuration, Kind, PackageManager};
pub use launch::Launch;
pub use menu::DEBUG_AT_POINT;
pub use process::{Event, Exit, Program, Starter};

pub struct RunPlugin;

const RUN: &str = "run";
const RUN_AT_POINT: &str = "run-at-point";
pub(crate) const CHOOSE: &str = "run-choose-configuration";
const STOP: &str = "run-stop";

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(RUN, "Run the project's chosen configuration, or the console's again", commands::run),
    Command::new(RUN_AT_POINT, "Run the entry point at point, saving a configuration for it the first time", commands::run_at_point),
    Command::new(CHOOSE, "Choose a configuration of the project and run it", chooser::choose),
    Command::new(STOP, "Stop the console's program, or the chosen configuration's", commands::stop),
    Command::new("run-edit-configurations", "Visit the project's run configurations", commands::edit_configurations),
    Command::new(chooser::EDIT, "Edit the selected configuration", chooser::edit),
    Command::new(window::ENTRY_POINT_MENU, "Show what can be done with the entry point at point", menu::open),
    Command::new(menu::NEXT, "Select the next item of the entry point menu", menu::next),
    Command::new(menu::PREVIOUS, "Select the previous item of the entry point menu", menu::previous),
    Command::new(menu::CHOOSE, "Do the selected item of the entry point menu", menu::choose),
    Command::new(menu::QUIT, "Close the entry point menu", menu::quit),
    Command::new("run-console-open-or-input", "Follow the link at point, else send the program a line", commands::open_link_or_input),
    Command::new("run-console-next-link", "Move to the next link in the console", commands::next_link),
    Command::new("run-console-previous-link", "Move to the previous link in the console", commands::previous_link),
    Command::new("run-console-stop", "Stop the console's program", commands::stop_console),
    Command::new("run-console-send-eof", "Close the program's input", commands::send_eof),
    Command::new("run-show-1", "Show the console of the first run", commands::show_session_1),
    Command::new("run-show-2", "Show the console of the second run", commands::show_session_2),
    Command::new("run-show-3", "Show the console of the third run", commands::show_session_3),
    Command::new("run-show-4", "Show the console of the fourth run", commands::show_session_4),
    Command::new("run-show-5", "Show the console of the fifth run", commands::show_session_5),
    Command::new("run-show-6", "Show the console of the sixth run", commands::show_session_6),
    Command::new("run-show-7", "Show the console of the seventh run", commands::show_session_7),
    Command::new("run-show-8", "Show the console of the eighth run", commands::show_session_8),
    Command::new("run-show-9", "Show the console of the ninth run", commands::show_session_9),
];

/// As in IntelliJ: ⌃R, ⌃⇧R, ⌥⇧R and ⌘F2; `keys.toml` binds them otherwise.
const BINDINGS: &[(&str, &str)] = &[
    ("C-r", RUN),
    ("C-R", RUN_AT_POINT),
    ("M-R", CHOOSE),
    ("s-<f2>", STOP),
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("Run", "run", "Run", RUN),
    MenuItem::new("Run", "run", "Run at Point", RUN_AT_POINT).enabled_when(visiting_file),
    MenuItem::new("Run", "run", "Choose Configuration…", CHOOSE),
    MenuItem::new("Run", "run", "Edit Configurations", "run-edit-configurations"),
    MenuItem::new("Run", "run", "Stop", STOP).enabled_when(sessions::any_running),
];

const MENU_BINDINGS: &[(&str, &str)] = &[
    ("C-n", menu::NEXT),
    ("<down>", menu::NEXT),
    ("C-p", menu::PREVIOUS),
    ("<up>", menu::PREVIOUS),
    ("RET", menu::CHOOSE),
    ("C-g", menu::QUIT),
    ("ESC", menu::QUIT),
];

const CONSOLE_BINDINGS: &[(&str, &str)] = &[
    ("RET", "run-console-open-or-input"),
    ("TAB", "run-console-next-link"),
    ("S-TAB", "run-console-previous-link"),
    ("C-c C-c", "run-console-stop"),
    ("C-c C-d", "run-console-send-eof"),
    ("M-1", "run-show-1"),
    ("M-2", "run-show-2"),
    ("M-3", "run-show-3"),
    ("M-4", "run-show-4"),
    ("M-5", "run-show-5"),
    ("M-6", "run-show-6"),
    ("M-7", "run-show-7"),
    ("M-8", "run-show-8"),
    ("M-9", "run-show-9"),
];

fn keymap(bindings: &[(&str, &'static str)]) -> Keymap {
    let mut keymap = Keymap::default();
    for &(keys, command) in bindings {
        keymap.bind(keys, command);
    }
    keymap
}

impl Plugin for RunPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend_from_slice(MENU_ITEMS);
        registry.minor_keymaps.push(MinorKeymap {
            active: menu::is_shown,
            keymap: keymap(MENU_BINDINGS),
        });
        registry.minor_keymaps.push(MinorKeymap {
            active: |editor| editor.window_buffer().local::<Console>().is_some(),
            keymap: keymap(CONSOLE_BINDINGS),
        });
        registry.settings_hooks.push(Rc::new(read_settings));
        registry.post_command_hooks.push(Rc::new(sessions::pick_up));
        registry.post_command_hooks.push(Rc::new(window::update));
        registry.post_command_hooks.push(Rc::new(menu::update));
        registry.decorators.push(Rc::new(console::style_spans));
        registry.decorators.push(Rc::new(console::link_spans));
        registry.gutters.push(Rc::new(window::gutter_marks));
        registry.popup_providers.push(Rc::new(menu::popup));
        registry.status_providers.push(Rc::new(status::status));
        registry.text_controls.push(Rc::new(status::controls));
    }
}

fn read_settings(editor: &mut Editor) {
    let settings = editor.settings_section::<RunSettings>();
    sessions::sessions(editor).settings = settings;
}

/// Keeps run files under `home`'s `run` directory, and which configuration each project
/// runs in its state directory.
pub fn keep_configurations_in(editor: &mut Editor, home: &Path) {
    configuration_store(editor).keep_in(run_dir(home), state_dir(home).join("run.json"));
}

fn configuration_store(editor: &mut Editor) -> &mut Store {
    editor.plugin_state()
}

#[cfg(test)]
mod tests;
