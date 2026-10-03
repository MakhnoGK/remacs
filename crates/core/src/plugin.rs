use crate::command::Commands;
use crate::keymap::Keymap;
use crate::mode::Modes;

/// Everything plugins contribute to the editor.
#[derive(Default)]
pub struct Registry {
    pub commands: Commands,
    pub keymap: Keymap,
    pub modes: Modes,
}

pub trait Plugin {
    fn register(&self, registry: &mut Registry);
}
