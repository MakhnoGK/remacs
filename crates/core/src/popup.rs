use std::rc::Rc;

use crate::completion::Candidate;
use crate::editor::Editor;

/// A list shown at a place in the window buffer, like Emacs corfu's popup; without rows,
/// the documentation alone, like an LSP hover.
pub struct Popup {
    /// Buffer offset the rows' labels or the documentation line up with, e.g. where the
    /// completed word starts.
    pub anchor: usize,
    /// Only the rows on screen.
    pub rows: Vec<Candidate>,
    pub selected: Option<usize>,
    /// About the selected row, shown beside the list.
    pub documentation: Option<Documentation>,
    /// Keys and what they do, under the documentation.
    pub hints: &'static [(&'static str, &'static str)],
}

/// Documentation of a symbol, e.g. a completion candidate's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Documentation {
    /// Where the symbol is declared, e.g. `core::ops::Range`.
    pub container: String,
    /// What the symbol is, e.g. `field`.
    pub kind: String,
    /// Its declaration, in the code font.
    pub signature: Option<String>,
    pub blocks: Vec<DocBlock>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocBlock {
    /// A paragraph; `quoted` parts are code.
    Text(String),
    Heading(String),
    Code(String),
}

impl Documentation {
    pub fn is_empty(&self) -> bool {
        self.signature.is_none() && self.blocks.is_empty()
    }
}

/// Describes a popup for the window buffer, if the plugin has one open.
pub type PopupProvider = Rc<dyn Fn(&Editor) -> Option<Popup>>;
