//! Built-in plugin: documentation of the symbol at point in a popup beside it, like an
//! LSP hover shown with Emacs `eldoc-doc-buffer`. It knows nothing of where documentation
//! comes from; backends such as `lsp` take its requests and deliver answers.

mod at_point;

use std::rc::Rc;

use remacs_core::{Command, Keymap, MinorKeymap, Plugin, Registry};

pub use at_point::{Answer, Request, deliver, take_requests};

pub struct DocumentationPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(at_point::SHOW, "Show the documentation of the symbol at point, then all of it", at_point::show),
    Command::new(at_point::HIDE, "Hide the documentation popup", at_point::hide),
];

const BINDINGS: &[(&str, &str)] = &[("C-h .", at_point::SHOW)];

/// Active while the popup is shown; any other command hides it too.
const POPUP_BINDINGS: &[(&str, &str)] = &[("ESC", at_point::HIDE)];

impl Plugin for DocumentationPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        let mut keymap = Keymap::default();
        for &(keys, command) in POPUP_BINDINGS {
            keymap.bind(keys, command);
        }
        registry.minor_keymaps.push(MinorKeymap {
            active: at_point::is_shown,
            keymap,
        });
        registry.post_command_hooks.push(Rc::new(at_point::update));
        registry.popup_providers.push(Rc::new(at_point::popup));
        registry.decorators.push(Rc::new(at_point::highlight));
    }
}

#[cfg(test)]
mod tests;
