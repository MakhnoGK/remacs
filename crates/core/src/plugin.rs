use std::rc::Rc;

use crate::command::Commands;
use crate::decoration::Decorator;
use crate::editor::Editor;
use crate::gutter::{GutterProvider, LineExtraProvider, TextControlsProvider};
use crate::keymap::Keymap;
use crate::mode::Modes;
use crate::outline::OutlineProvider;
use crate::pane::PaneSource;
use crate::popup::{CodeHighlighter, PopupProvider};
use crate::screen::{KeySink, ScreenProvider};
use crate::status::StatusProvider;
use crate::syntax::SyntaxProbe;
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
    /// While one holds, typed characters no keymap binds are not inserted, e.g. while a
    /// pane has the keys.
    pub typing_blockers: Vec<fn(&Editor) -> bool>,
    pub modes: Modes,
    /// Run after every command and whenever background work has results, like Emacs
    /// `post-command-hook`.
    pub post_command_hooks: Vec<Hook>,
    /// Run when the settings are read: at startup and whenever their files change. Each
    /// reads its plugin's section with `Editor::settings_section`.
    pub settings_hooks: Vec<Hook>,
    pub decorators: Vec<Decorator>,
    /// Tell strings and comments from code; the first that knows the buffer does.
    pub syntax_probes: Vec<SyntaxProbe>,
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
    /// Marks beside the lines of buffers drawn as text, all plugins' together.
    pub gutters: Vec<GutterProvider>,
    /// Run by a click beside a line that has no mark, after point moved to it, e.g. to
    /// add a breakpoint there.
    pub gutter_click: Option<&'static str>,
    /// Bands behind lines of buffers drawn as text, and notes after them.
    pub line_extras: Vec<LineExtraProvider>,
    /// Panels under the window buffer, sharing one slot.
    pub panes: Vec<PaneSource>,
    /// Draw buffers as screens; the first that knows the buffer does.
    pub screens: Vec<ScreenProvider>,
    /// Take keys before the keymaps, in order, while no prefix or prompt is pending.
    pub key_sinks: Vec<KeySink>,
    /// Controls of buffers drawn as text; the first that knows the buffer has them.
    pub text_controls: Vec<TextControlsProvider>,
    /// For each file, the first that controls it.
    pub version_controls: Vec<Rc<dyn VersionControl>>,
}

pub trait Plugin {
    fn register(&self, registry: &mut Registry);
}
