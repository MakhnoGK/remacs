//! Built-in plugin: going to the definition of the symbol at point and back, to its uses,
//! implementations and type, through its call and type hierarchies, to the parent module
//! and package manifest of the code, and to any symbol of the project by name, like Emacs
//! xref. It knows nothing of where definitions and
//! symbols come from; backends such as `lsp` take its requests and deliver locations.

mod apropos;
mod definitions;
mod hierarchy;
mod imenu;

use std::rc::Rc;

use remacs_core::{
    Command, Keymap, MenuItem, MinorKeymap, PaneSource, Plugin, Registry, visiting_file,
};

pub use apropos::{Symbol, SymbolRequest, deliver_symbols, take_symbol_requests};
pub use definitions::{
    FIND_IMPLEMENTATIONS, FIND_MANIFEST, FIND_PARENT_MODULE, FIND_TYPE_DEFINITIONS, Location,
    Lookup, Request, deliver, refuse, take_requests,
};
pub use hierarchy::{
    CALL_HIERARCHY, Hierarchy, HierarchyItem, HierarchyRequest, Relation, TYPE_HIERARCHY,
    deliver_hierarchy, take_hierarchy_requests,
};
pub use imenu::{DocumentSymbolRequest, deliver_document_symbols, take_document_symbol_requests};

pub struct XrefPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(definitions::FIND_DEFINITIONS, "Go to the definition of the symbol at point", definitions::find_definitions),
    Command::new(definitions::FIND_REFERENCES, "List the uses of the symbol at point and go to one", definitions::find_references),
    Command::new(FIND_IMPLEMENTATIONS, "Go to an implementation of the trait, interface or method at point", definitions::find_implementations),
    Command::new(FIND_TYPE_DEFINITIONS, "Go to the definition of the type of the value at point", definitions::find_type_definitions),
    Command::new(FIND_PARENT_MODULE, "Go to where the module of the code at point is declared, like Rust's `mod`", definitions::find_parent_module),
    Command::new(FIND_MANIFEST, "Go to the manifest of the package of the code at point, like its Cargo.toml", definitions::find_manifest),
    Command::new("xref-go-back", "Go back to where the last definition was looked up", definitions::go_back),
    Command::new(apropos::FIND_APROPOS, "Find a symbol of the project by name and go to it", apropos::find_apropos),
    Command::new(imenu::IMENU, "Go to a symbol of the file by name", imenu::imenu),
];

const BINDINGS: &[(&str, &str)] = &[
    ("M-.", definitions::FIND_DEFINITIONS),
    ("M-?", definitions::FIND_REFERENCES),
    ("M-g M-i", FIND_IMPLEMENTATIONS),
    ("M-g M-t", FIND_TYPE_DEFINITIONS),
    ("M-g M-c", CALL_HIERARCHY),
    ("M-g M-y", TYPE_HIERARCHY),
    ("M-,", "xref-go-back"),
    ("C-M-.", apropos::FIND_APROPOS),
    ("M-g i", imenu::IMENU),
    ("s-<f12>", imenu::IMENU),
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("Go", "definitions", "Definition", definitions::FIND_DEFINITIONS).enabled_when(visiting_file),
    MenuItem::new("Go", "definitions", "References", definitions::FIND_REFERENCES).enabled_when(visiting_file),
    MenuItem::new("Go", "definitions", "Back", "xref-go-back"),
    MenuItem::new("Go", "definitions", "Symbol in File…", imenu::IMENU).enabled_when(visiting_file),
    MenuItem::new("Go", "definitions", "Symbol in Project…", apropos::FIND_APROPOS),
];

impl Plugin for XrefPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS.iter().chain(hierarchy::COMMANDS) {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        let mut focused = Keymap::default();
        for &(keys, command) in hierarchy::FOCUSED_BINDINGS {
            focused.bind(keys, command);
        }
        registry.minor_keymaps.push(MinorKeymap { active: hierarchy::has_keys, keymap: focused });
        registry.typing_blockers.push(hierarchy::has_keys);
        registry.panes.push(PaneSource {
            name: hierarchy::NAME,
            shown: hierarchy::is_shown,
            describe: Rc::new(hierarchy::pane),
        });
        registry.settings_hooks.push(Rc::new(hierarchy::read_settings));
        registry.menu_items.extend_from_slice(MENU_ITEMS);
        registry.post_command_hooks.push(Rc::new(definitions::update));
        registry.post_command_hooks.push(Rc::new(apropos::update));
        registry.post_command_hooks.push(Rc::new(imenu::update));
    }
}

#[cfg(test)]
mod tests;
