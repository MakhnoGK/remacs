use std::rc::Rc;

use crate::command::Commands;
use crate::decoration::Decorator;
use crate::editor::Editor;
use crate::keymap::Keymap;
use crate::mode::Modes;
use crate::outline::OutlineProvider;
use crate::popup::{CodeHighlighter, PopupProvider};
use crate::status::StatusProvider;
use crate::table::TableProvider;
use crate::vc::VersionControl;

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
    /// Active over `minibuffer_keymap` while a prompt edits a message.
    pub message_keymap: Keymap,
    pub minor_keymaps: Vec<MinorKeymap>,
    pub modes: Modes,
    /// Run after every command and whenever background work has results, like Emacs
    /// `post-command-hook`.
    pub post_command_hooks: Vec<Hook>,
    pub decorators: Vec<Decorator>,
    /// Segments of the status pill, before the position and the mode.
    pub status_providers: Vec<StatusProvider>,
    pub popup_providers: Vec<PopupProvider>,
    /// Highlight code that is not in a buffer, e.g. in documentation; the first that
    /// knows the mode does.
    pub code_highlighters: Vec<CodeHighlighter>,
    /// Draw buffers as tables; the first that knows the buffer does.
    pub tables: Vec<TableProvider>,
    /// Draw buffers as outlines; the first that knows the buffer does.
    pub outlines: Vec<OutlineProvider>,
    /// For each file, the first that controls it.
    pub version_controls: Vec<Rc<dyn VersionControl>>,
}

pub trait Plugin {
    fn register(&self, registry: &mut Registry);
}
