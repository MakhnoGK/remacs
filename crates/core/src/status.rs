use std::rc::Rc;

use crate::completion::Icon;
use crate::editor::Editor;

/// A group of the status pill that a plugin contributes, like an Emacs mode-line
/// construct; clicking it runs `command`, if any.
pub struct StatusSegment {
    pub items: Vec<StatusItem>,
    pub command: Option<&'static str>,
    /// Drawn pressed, e.g. while the list it toggles is open.
    pub active: bool,
    /// Drawn in the code font, e.g. counts or a branch name; prose is not.
    pub code: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusItem {
    pub icon: Option<Icon>,
    pub text: String,
    pub tone: Tone,
}

impl StatusItem {
    pub fn new(icon: Option<Icon>, text: impl Into<String>) -> Self {
        Self {
            icon,
            text: text.into(),
            tone: Tone::Plain,
        }
    }

    pub fn toned(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            tone,
            ..Self::new(None, text)
        }
    }
}

/// What a piece of text stands out as; the UI maps tones to theme colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Plain,
    /// Chosen, like marked files.
    Accent,
    /// About to be destroyed, like files flagged for deletion.
    Danger,
    /// Not yet done, like edits waiting to be applied.
    Warning,
    /// Secondary, like counts beside a name.
    Faint,
    /// Set apart, like commits folded into others.
    Special,
    /// Done, like conflicts resolved.
    Success,
}

/// Describes the window buffer for the status pill, if the plugin has anything to show.
pub type StatusProvider = Rc<dyn Fn(&Editor) -> Option<StatusSegment>>;
