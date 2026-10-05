//! Built-in plugin: searching the buffer, its matching lines listed in a prompt like the
//! other searches, like Emacs `consult-line`.

mod lines;
mod query;

use remacs_core::{Command, Plugin, Registry};

pub struct SearchPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(lines::SEARCH, "Search the buffer, listing the lines that match", lines::search),
];

const BINDINGS: &[(&str, &str)] = &[("C-s", lines::SEARCH)];

impl Plugin for SearchPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
    }
}

#[cfg(test)]
mod tests;
