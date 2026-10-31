//! Built-in plugin: documentation of the symbol at point in a popup beside it, like an
//! LSP hover shown with Emacs `eldoc-doc-buffer`, and the signature of the call being
//! typed above it, like LSP signature help. It knows nothing of where either comes from;
//! backends such as `lsp` take its requests and deliver answers.

mod at_point;
mod signature;

use std::rc::Rc;

use remacs_core::{
    Command, Editor, Keymap, MenuItem, MinorKeymap, Plugin, Registry, visiting_file,
};

pub use at_point::{Answer, Request, deliver, take_requests};
pub use signature::{
    SignatureRequest, SignatureTrigger, Signatures, deliver_signatures, take_signature_requests,
};

pub struct DocumentationPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(at_point::SHOW, "Show the documentation of the symbol at point, then all of it", at_point::show),
    Command::new(at_point::HIDE, "Hide the documentation popup", at_point::hide),
    Command::new(at_point::SCROLL, "Scroll the documentation popup a page forward", at_point::scroll),
    Command::new(at_point::SCROLL_DOWN, "Scroll the documentation popup a page back", at_point::scroll_down),
    Command::new(signature::SHOW, "Show the signature of the call at point, then its next overload", signature::show),
    Command::new(signature::HIDE, "Hide the signature popup", signature::hide),
];

const BINDINGS: &[(&str, &str)] = &[("C-h .", at_point::SHOW), ("s-p", signature::SHOW)];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("Help", "help", "Documentation at Point", at_point::SHOW).enabled_when(visiting_file),
    MenuItem::new("Help", "help", "Signature at Point", signature::SHOW).enabled_when(visiting_file),
];

/// Active while the popup is shown; any other command hides it too.
const POPUP_BINDINGS: &[(&str, &str)] =
    &[("ESC", at_point::HIDE), ("C-M-v", at_point::SCROLL), ("C-M-V", at_point::SCROLL_DOWN)];

/// Active while a signature is shown, which typing and moving within the call keep.
const SIGNATURE_BINDINGS: &[(&str, &str)] = &[("ESC", signature::HIDE)];

impl Plugin for DocumentationPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend_from_slice(MENU_ITEMS);
        let minor_keymaps = [
            (at_point::is_shown as fn(&Editor) -> bool, POPUP_BINDINGS),
            (signature::is_shown, SIGNATURE_BINDINGS),
        ];
        for (active, bindings) in minor_keymaps {
            let mut keymap = Keymap::default();
            for &(keys, command) in bindings {
                keymap.bind(keys, command);
            }
            registry.minor_keymaps.push(MinorKeymap { active, keymap });
        }
        registry.post_command_hooks.push(Rc::new(at_point::update));
        registry.post_command_hooks.push(Rc::new(signature::update));
        registry.popup_providers.push(Rc::new(at_point::popup));
        registry.popup_providers.push(Rc::new(signature::popup));
        registry.decorators.push(Rc::new(at_point::highlight));
    }
}

#[cfg(test)]
mod tests;
