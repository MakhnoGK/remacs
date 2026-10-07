use std::rc::Rc;

use crate::editor::Editor;
use crate::status::Tone;
use crate::table::Action;

/// A panel under the window buffer, like an Emacs side window at the bottom: a plugin's
/// view of its own state in columns of rows, e.g. a debugger's frames and variables.
pub struct Pane {
    /// Pixels, from the plugin's settings.
    pub height: f32,
    /// E.g. the configuration debugged, and what debugs it.
    pub title: String,
    pub subtitle: String,
    /// How it goes, e.g. `Paused · breakpoint`.
    pub state: (String, Tone),
    pub buttons: Vec<PaneButton>,
    pub columns: Vec<PaneColumn>,
    /// Keys and what they do, under the window in place of the buffer's.
    pub hints: &'static [(&'static str, &'static str)],
}

pub struct PaneButton {
    pub label: &'static str,
    /// Shown beside the label, e.g. `⇧F8`.
    pub keys: &'static str,
    pub enabled: bool,
    pub action: Action,
}

pub struct PaneColumn {
    pub title: &'static str,
    /// The keys that move to it, beside the title.
    pub keys: &'static str,
    /// Pixels; `None` takes the room the others leave.
    pub width: Option<f32>,
    pub rows: Vec<PaneRow>,
    /// Keys moves through its rows from here, while it has the keys.
    pub selected: Option<usize>,
    pub focused: bool,
    /// Said in place of rows when there are none.
    pub empty: String,
    /// Keys and what they do here, beside the title while it has the keys.
    pub hints: &'static [(&'static str, &'static str)],
}

#[derive(Clone, Default)]
pub struct PaneRow {
    /// Nesting, e.g. of a variable's fields.
    pub depth: usize,
    /// Whether it opens, and is open.
    pub open: Option<bool>,
    pub name: String,
    /// What it is, e.g. a variable's type, in the faint code font after the name.
    pub kind: String,
    /// E.g. a variable's value, after `=`.
    pub value: Option<String>,
    /// On the right, e.g. where a frame is.
    pub detail: String,
    /// A chip after the name, e.g. `breakpoint`.
    pub tag: Option<(String, Tone)>,
    /// Drawn fainter, e.g. frames of libraries; or as the one that matters, e.g. the
    /// frame shown.
    pub tone: Tone,
    pub current: bool,
    /// Run by a click on it.
    pub action: Option<Action>,
}

/// Describes the pane, if the plugin has one to show.
pub type PaneProvider = Rc<dyn Fn(&Editor) -> Option<Pane>>;
