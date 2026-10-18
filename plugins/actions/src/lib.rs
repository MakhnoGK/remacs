//! Built-in plugin: code actions at point, such as quick fixes and refactorings, in a
//! popup beside it, like Emacs `eglot-code-actions`. It knows nothing of where actions
//! come from; backends such as `lsp` take its requests, deliver actions and the edits
//! the chosen one makes.

mod edits;
mod menu;

use std::rc::Rc;

use remacs_core::{Command, Keymap, MenuItem, MinorKeymap, Plugin, Registry, visiting_file};

pub use edits::{apply, failed};
pub use menu::{Action, Request, deliver, take_requests};

pub struct ActionsPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(menu::CODE_ACTIONS, "Show the code actions at point or for the region", menu::code_actions),
    Command::new(menu::NEXT, "Select the next code action", menu::next),
    Command::new(menu::PREVIOUS, "Select the previous code action", menu::previous),
    Command::new(menu::APPLY, "Apply the selected code action", menu::apply_selected),
    Command::new(menu::QUIT, "Close the code actions popup", menu::quit),
    Command::new(menu::FIX_ALL, "Apply the fixes the servers offer for the whole buffer", menu::fix_all),
];

const BINDINGS: &[(&str, &str)] = &[("C-c C-a", menu::CODE_ACTIONS)];

const MENU_ITEMS: [MenuItem; 2] = [
    MenuItem::new("Edit", "code", "Code Actions…", menu::CODE_ACTIONS).enabled_when(visiting_file),
    MenuItem::new("Edit", "code", "Fix All", menu::FIX_ALL).enabled_when(visiting_file),
];

/// Active while the popup is shown; any other command closes it.
const POPUP_BINDINGS: &[(&str, &str)] = &[
    ("C-n", menu::NEXT),
    ("<down>", menu::NEXT),
    ("C-p", menu::PREVIOUS),
    ("<up>", menu::PREVIOUS),
    ("RET", menu::APPLY),
    ("C-g", menu::QUIT),
    ("ESC", menu::QUIT),
];

impl Plugin for ActionsPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend(MENU_ITEMS);
        let mut keymap = Keymap::default();
        for &(keys, command) in POPUP_BINDINGS {
            keymap.bind(keys, command);
        }
        registry.minor_keymaps.push(MinorKeymap {
            active: menu::is_shown,
            keymap,
        });
        registry.post_command_hooks.push(Rc::new(menu::update));
        registry.popup_providers.push(Rc::new(menu::popup));
    }
}

#[cfg(test)]
mod tests;
