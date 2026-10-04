//! Built-in plugin: completion at point in a popup beside the word, like Emacs corfu. It
//! knows nothing of where candidates come from; backends such as `lsp` take its requests
//! and deliver lists.

mod backend;
mod session;

use std::rc::Rc;

use remacs_core::{Command, Keymap, MinorKeymap, Plugin, Registry};

pub use backend::{Item, List, Request, Trigger, deliver, is_word_char, resolved, take_requests};

pub struct CompletionPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(session::COMPLETION_AT_POINT, "Complete the word at point", session::completion_at_point),
    Command::new(session::NEXT, "Select the next completion", session::next),
    Command::new(session::PREVIOUS, "Select the previous completion", session::previous),
    Command::new(session::INSERT, "Insert the selected completion", session::insert),
    Command::new(session::QUIT, "Close the completion popup", session::quit),
];

const BINDINGS: &[(&str, &str)] = &[
    ("C-M-i", session::COMPLETION_AT_POINT),
    ("M-TAB", session::COMPLETION_AT_POINT),
];

/// Active while the popup is shown, like `corfu-map`.
const POPUP_BINDINGS: &[(&str, &str)] = &[
    ("C-n", session::NEXT),
    ("<down>", session::NEXT),
    ("C-p", session::PREVIOUS),
    ("<up>", session::PREVIOUS),
    ("RET", session::INSERT),
    ("TAB", session::INSERT),
    ("C-g", session::QUIT),
    ("ESC", session::QUIT),
];

impl Plugin for CompletionPlugin {
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
            active: session::is_shown,
            keymap,
        });
        registry.post_command_hooks.push(Rc::new(session::update));
        registry.popup_providers.push(Rc::new(session::popup));
    }
}

#[cfg(test)]
mod tests;
