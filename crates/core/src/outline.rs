use std::ops::Range;
use std::rc::Rc;

use crate::editor::Editor;
use crate::status::Tone;
use crate::table::Button;
use crate::vc::{FileState, LineCounts};

/// A buffer drawn as a pane of sections, one row per line, like Emacs magit-status:
/// headings that fold, files under them and, unfolded, a file's diff. Point's line is
/// the selected row.
pub struct Outline {
    /// Facts above the rows, e.g. the branch checked out.
    pub facts: Vec<Fact>,
    /// Only the rows of the lines asked for.
    pub rows: Vec<OutlineRow>,
    /// Run by a click on a heading, after point moved to it.
    pub toggle: &'static str,
    /// Run by a double click on a row, after point moved to it.
    pub activate: &'static str,
    /// Keys and what they do, under the pane.
    pub hints: &'static [(&'static str, &'static str)],
}

/// A labelled fact, e.g. `Head` and the branch, with what else there is to say of it.
pub struct Fact {
    pub label: &'static str,
    pub value: FactValue,
    /// After the value, e.g. `2 commits to push`.
    pub note: String,
    /// The note says what the value is, e.g. a commit's subject, so it is drawn brighter.
    pub note_is_title: bool,
    pub buttons: Vec<Button>,
}

pub enum FactValue {
    Ref(RefChip),
    /// E.g. a remote branch, in the code font.
    Code(String),
}

/// A branch or tag drawn as a chip, colored by what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefChip {
    pub name: String,
    pub kind: RefKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    /// Checked out.
    Head,
    /// What branches are based on, like `main`.
    Main,
    Branch,
    Remote,
    Tag,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutlineRow {
    Heading {
        title: String,
        count: usize,
        open: bool,
        /// The count's, e.g. red for unresolved conflicts.
        tone: Tone,
    },
    File {
        state: FileState,
        name: String,
        /// Where it is, e.g. `src/`.
        directory: String,
        lines: Option<LineCounts>,
        open: bool,
    },
    /// A line of an open file's diff.
    Diff(DiffRow),
    /// Something else under a heading, e.g. a stash: a name and what it is.
    Item { name: String, description: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffRow {
    /// `@@ -39,3 +39,6 @@ pub fn insert`
    Hunk(String),
    Context(String),
    Added(String),
    Removed(String),
}

/// Describes `lines` of the window buffer as an outline, if the plugin draws that buffer.
pub type OutlineProvider = Rc<dyn Fn(&Editor, Range<usize>) -> Option<Outline>>;
