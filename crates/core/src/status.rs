use std::rc::Rc;
use std::time::Instant;

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
    pub spinner: Option<Spinner>,
}

impl StatusItem {
    pub fn new(icon: Option<Icon>, text: impl Into<String>) -> Self {
        Self { icon, text: text.into(), tone: Tone::Plain, spinner: None }
    }

    pub fn toned(text: impl Into<String>, tone: Tone) -> Self {
        Self { tone, ..Self::new(None, text) }
    }

    /// Work going on since `since`, shown only once it has lasted a while.
    pub fn working(text: impl Into<String>, since: Instant, percent: Option<u8>) -> Self {
        Self { spinner: Some(Spinner { since, percent }), ..Self::new(None, text) }
    }
}

/// A ring before an item's text: filled as far as `percent`, or an arc turning while
/// how far the work is is unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spinner {
    pub since: Instant,
    pub percent: Option<u8>,
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
