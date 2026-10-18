use std::ops::Range;
use std::rc::Rc;

use crate::editor::Editor;
use crate::outline::ToolbarButton;
use crate::pane::{PaneButton, Tabs};
use crate::status::Tone;

/// Marks of one line are drawn in this order, the later over the earlier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GutterIcon {
    /// The line starts something that can be run, like a program's `main`.
    Run,
    /// A program being debugged stops at the line.
    Breakpoint(BreakpointIcon),
    /// A paused program is at the line; drawn over its other mark.
    ExecutionPoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BreakpointIcon {
    Plain,
    /// Stops only when a condition holds, or after so many hits.
    Conditional,
    /// Logs a message instead of stopping.
    Log,
    Disabled,
    /// The debugger found no code at the line.
    Unverified,
}

/// A mark beside a line of the window buffer; a click puts point on the line and runs
/// `command`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GutterMark {
    pub line: usize,
    pub icon: GutterIcon,
    pub command: &'static str,
}

/// Marks beside `lines` of the window buffer; none when the plugin has nothing to mark in
/// the buffer at all, so the gutter makes room for marks only where some plugin might.
pub type GutterProvider = Rc<dyn Fn(&Editor, Range<usize>) -> Option<Vec<GutterMark>>>;

/// More drawn with a line of the window buffer, like a paused program's line and the
/// values of the variables on it: a band behind it, and a note after its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineExtra {
    pub line: usize,
    pub band: Option<Tone>,
    /// Drawn faint, in the code font.
    pub note: Option<String>,
}

/// Extras of `lines` of the window buffer, all plugins' together.
pub type LineExtraProvider = Rc<dyn Fn(&Editor, Range<usize>) -> Vec<LineExtra>>;

/// Text drawn over the window buffer's text from `offset` on, like the labels Emacs `avy`
/// puts on what can be jumped to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextLabel {
    pub offset: usize,
    pub text: String,
}

/// Labels on `lines` of the window buffer, all plugins' together.
pub type TextLabelProvider = Rc<dyn Fn(&Editor, Range<usize>) -> Vec<TextLabel>>;

/// Controls of a buffer drawn as text or as a screen, like an Emacs header line: buttons
/// in the title bar and keys under the text, e.g. a process's Stop and Rerun.
#[derive(Default)]
pub struct TextControls {
    pub toolbar: Vec<ToolbarButton>,
    /// Labelled with their keys, after the toolbar.
    pub buttons: Vec<PaneButton>,
    /// On the left of the title bar, e.g. one per terminal.
    pub tabs: Option<Tabs>,
    pub hints: &'static [(&'static str, &'static str)],
}

/// Controls for the window buffer, if the plugin has any for it.
pub type TextControlsProvider = Rc<dyn Fn(&Editor) -> Option<TextControls>>;
