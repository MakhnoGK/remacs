//! Built-in plugin: going to the definition of the symbol at point and back, like Emacs
//! xref. It knows nothing of where definitions come from; backends such as `lsp` take its
//! requests and deliver locations.

mod definitions;

use std::rc::Rc;

use remacs_core::{Command, Plugin, Registry};

pub use definitions::{Location, Request, deliver, take_requests};

pub struct XrefPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(definitions::FIND_DEFINITIONS, "Go to the definition of the symbol at point", definitions::find_definitions),
    Command::new("xref-go-back", "Go back to where the last definition was looked up", definitions::go_back),
];

const BINDINGS: &[(&str, &str)] = &[
    ("M-.", definitions::FIND_DEFINITIONS),
    ("M-,", "xref-go-back"),
];

impl Plugin for XrefPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry
            .post_command_hooks
            .push(Rc::new(definitions::update));
    }
}

#[cfg(test)]
mod tests;
