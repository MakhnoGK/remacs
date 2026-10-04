use std::rc::Rc;

use crate::command::Commands;
use crate::decoration::Decorator;
use crate::editor::Editor;
use crate::keymap::Keymap;
use crate::mode::Modes;
use crate::popup::PopupProvider;
use crate::status::StatusProvider;

pub type Hook = Rc<dyn Fn(&mut Editor)>;

/// A keymap active over the global one while `active` holds, like an Emacs minor mode map.
pub struct MinorKeymap {
    pub active: fn(&Editor) -> bool,
    pub keymap: Keymap,
}

/// Everything plugins contribute to the editor.
#[derive(Default)]
pub struct Registry {
    pub commands: Commands,
    pub keymap: Keymap,
    /// Active over `keymap` while a prompt is open, like `minibuffer-local-map`.
    pub minibuffer_keymap: Keymap,
    pub minor_keymaps: Vec<MinorKeymap>,
    pub modes: Modes,
    /// Run after every command and whenever background work has results, like Emacs
    /// `post-command-hook`.
    pub post_command_hooks: Vec<Hook>,
    pub decorators: Vec<Decorator>,
    /// Segments of the status pill, before the position and the mode.
    pub status_providers: Vec<StatusProvider>,
    pub popup_providers: Vec<PopupProvider>,
}

pub trait Plugin {
    fn register(&self, registry: &mut Registry);
}
