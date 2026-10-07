use std::ops::Range;
use std::rc::Rc;

use crate::editor::Editor;
use crate::outline::ToolbarButton;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GutterIcon {
    /// The line starts something that can be run, like a program's `main`.
    Run,
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

/// Controls of a buffer drawn as text, like an Emacs header line: buttons in the title
/// bar and keys under the text, e.g. a process's Stop and Rerun.
#[derive(Default)]
pub struct TextControls {
    pub toolbar: Vec<ToolbarButton>,
    pub hints: &'static [(&'static str, &'static str)],
}

/// Controls for the window buffer, if the plugin has any for it.
pub type TextControlsProvider = Rc<dyn Fn(&Editor) -> Option<TextControls>>;
