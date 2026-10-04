use crate::command::Commands;
use crate::keymap::Keymap;
use crate::mode::Modes;

/// Everything plugins contribute to the editor.
#[derive(Default)]
pub struct Registry {
    pub commands: Commands,
    pub keymap: Keymap,
    /// Active over `keymap` while a prompt is open, like `minibuffer-local-map`.
    pub minibuffer_keymap: Keymap,
    pub modes: Modes,
}

pub trait Plugin {
    fn register(&self, registry: &mut Registry);
}
