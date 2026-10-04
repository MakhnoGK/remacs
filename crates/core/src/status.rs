use std::rc::Rc;

use crate::completion::Icon;
use crate::editor::Editor;

/// A group of the status pill that a plugin contributes, like an Emacs mode-line
/// construct; clicking it runs `command`, if any.
pub struct StatusSegment {
    pub items: Vec<(Option<Icon>, String)>,
    pub command: Option<&'static str>,
    /// Drawn pressed, e.g. while the list it toggles is open.
    pub active: bool,
}

/// Describes the window buffer for the status pill, if the plugin has anything to show.
pub type StatusProvider = Rc<dyn Fn(&Editor) -> Option<StatusSegment>>;
