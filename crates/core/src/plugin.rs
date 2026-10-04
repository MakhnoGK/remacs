use std::rc::Rc;

use crate::command::Commands;
use crate::decoration::Decorator;
use crate::editor::Editor;
use crate::keymap::Keymap;
use crate::mode::Modes;
use crate::status::StatusProvider;

pub type Hook = Rc<dyn Fn(&mut Editor)>;

/// Everything plugins contribute to the editor.
#[derive(Default)]
pub struct Registry {
    pub commands: Commands,
    pub keymap: Keymap,
    /// Active over `keymap` while a prompt is open, like `minibuffer-local-map`.
    pub minibuffer_keymap: Keymap,
    pub modes: Modes,
    /// Run after every command and whenever background work has results, like Emacs
    /// `post-command-hook`.
    pub post_command_hooks: Vec<Hook>,
    pub decorators: Vec<Decorator>,
    /// Segments of the status pill, before the position and the mode.
    pub status_providers: Vec<StatusProvider>,
}

pub trait Plugin {
    fn register(&self, registry: &mut Registry);
}
