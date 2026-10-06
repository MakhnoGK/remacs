//! Built-in plugin: going to the definition of the symbol at point and back, and to any
//! symbol of the project by name, like Emacs xref. It knows nothing of where definitions
//! and symbols come from; backends such as `lsp` take its requests and deliver locations.

mod apropos;
mod definitions;

use std::rc::Rc;

use remacs_core::{Command, Plugin, Registry};

pub use apropos::{Symbol, SymbolRequest, deliver_symbols, take_symbol_requests};
pub use definitions::{Location, Request, deliver, take_requests};

pub struct XrefPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(definitions::FIND_DEFINITIONS, "Go to the definition of the symbol at point", definitions::find_definitions),
    Command::new("xref-go-back", "Go back to where the last definition was looked up", definitions::go_back),
    Command::new(apropos::FIND_APROPOS, "Find a symbol of the project by name and go to it", apropos::find_apropos),
];

const BINDINGS: &[(&str, &str)] = &[
    ("M-.", definitions::FIND_DEFINITIONS),
    ("M-,", "xref-go-back"),
    ("C-M-.", apropos::FIND_APROPOS),
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
        registry.post_command_hooks.push(Rc::new(apropos::update));
    }
}

#[cfg(test)]
mod tests;
