use std::rc::Rc;

use crate::editor::Editor;
use crate::status::Tone;
use crate::table::Action;

/// A panel under the window buffer, like an Emacs side window at the bottom: a plugin's
/// view of its own state, e.g. a debugger's frames and variables, or a terminal. Panes
/// share one slot; a switch in the toolbar picks the one shown.
pub struct Pane {
    /// Beside the name in the switch, e.g. how many terminals there are.
    pub count: Option<usize>,
    /// Pixels, from the plugin's settings.
    pub height: f32,
    /// E.g. the configuration debugged, and what debugs it.
    pub title: String,
    pub subtitle: String,
    /// How it goes, e.g. `Paused · breakpoint`.
    pub state: (String, Tone),
    /// Before the title, e.g. one per terminal.
    pub tabs: Option<Tabs>,
    pub buttons: Vec<PaneButton>,
    pub body: PaneBody,
    /// Keys and what they do, under the window in place of the buffer's.
    pub hints: &'static [(&'static str, &'static str)],
}

pub enum PaneBody {
    Columns(Vec<PaneColumn>),
    /// A buffer drawn as a screen, by its name.
    Screen(String),
}

#[derive(Clone)]
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

/// Chips for buffers of one kind, like terminals: a click shows one.
#[derive(Clone)]
pub struct Tabs {
    pub tabs: Vec<Tab>,
    /// Lists them all, from a chip in place of those that do not fit.
    pub list: &'static str,
    /// Adds one, from the `+` after them.
    pub add: &'static str,
}

#[derive(Clone)]
pub struct Tab {
    pub label: String,
    pub selected: bool,
    pub state: TabState,
    /// Something new was printed since it was last shown.
    pub activity: bool,
    /// Its program rang the bell since it was last shown.
    pub bell: bool,
    /// Renamed in place: the name typed so far.
    pub editing: Option<String>,
    pub action: Action,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabState {
    #[default]
    Idle,
    Running,
    Exited(i32),
}

/// Describes the pane.
pub type PaneProvider = Rc<dyn Fn(&Editor) -> Option<Pane>>;

/// A plugin's pane: `shown` tells cheaply whether it has anything to show.
#[derive(Clone)]
pub struct PaneSource {
    /// Names it in the switch, e.g. `Debug`.
    pub name: &'static str,
    pub shown: fn(&Editor) -> bool,
    pub describe: PaneProvider,
}
