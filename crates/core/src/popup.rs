use std::ops::Range;
use std::rc::Rc;

use crate::completion::{Candidate, Icon};
use crate::decoration::Span;
use crate::editor::Editor;

/// A list shown at a place in the window buffer, like Emacs corfu's popup; without rows,
/// the documentation alone, like an LSP hover, or notes about the text, like problems.
#[derive(Default)]
pub struct Popup {
    /// Buffer offset the rows' labels or the documentation line up with, e.g. where the
    /// completed word starts.
    pub anchor: usize,
    /// Only the rows on screen.
    pub rows: Vec<Candidate>,
    pub selected: Option<usize>,
    /// About the selected row, shown beside the list.
    pub documentation: Option<Documentation>,
    /// Pages the documentation was scrolled by keys since it was shown, back ones
    /// negative, like `scroll-other-window`; the mouse wheel scrolls it too.
    pub documentation_pages: i32,
    /// Keys and what they do, under the documentation.
    pub hints: &'static [(&'static str, &'static str)],
    /// About the text at the anchor, shown without rows or documentation.
    pub notes: Vec<Note>,
    /// Documentation goes above the line when there is room, e.g. a signature, so that
    /// completions below stay in view.
    pub above: bool,
}

/// A remark about text in the buffer, like a problem a checker reported there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub icon: Icon,
    /// `quoted` parts are code.
    pub message: String,
    /// Who made it, e.g. `rustc · E0599`.
    pub source: String,
}

/// Documentation of a symbol, e.g. a completion candidate's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Documentation {
    /// Where the symbol is declared, e.g. `core::ops::Range`.
    pub container: String,
    /// What the symbol is, e.g. `field`.
    pub kind: String,
    /// Its declaration.
    pub signature: Option<Code>,
    pub blocks: Vec<DocBlock>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocBlock {
    /// A paragraph; `quoted` parts are code.
    Text(String),
    Heading(String),
    Code(Code),
}

/// Code in documentation, highlighted as the major mode it is written in, if known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    pub text: String,
    pub mode: Option<&'static str>,
    /// Bytes of `text` that stand out, e.g. the parameter of a signature being typed.
    pub emphasis: Option<Range<usize>>,
}

impl Code {
    pub fn new(text: impl Into<String>, mode: Option<&'static str>) -> Self {
        Self {
            text: text.into(),
            mode,
            emphasis: None,
        }
    }
}

impl Documentation {
    pub fn is_empty(&self) -> bool {
        self.signature.is_none() && self.blocks.is_empty()
    }
}

/// Describes a popup for the window buffer, if the plugin has one open.
pub type PopupProvider = Rc<dyn Fn(&Editor) -> Option<Popup>>;

/// Faces for `code` written in a major mode, if the highlighter knows the mode.
pub type CodeHighlighter = Rc<dyn Fn(&'static str, &str) -> Option<Vec<Span>>>;
