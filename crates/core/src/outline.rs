use std::ops::Range;
use std::rc::Rc;

use crate::command::CommandResult;
use crate::completion::Icon;
use crate::editor::Editor;
use crate::gutter::TextSide;
use crate::status::Tone;
use crate::table::{Action, Button};
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
    /// Beside the pane when there is room.
    pub history: Option<History>,
    /// Beside the pane in place of the history, e.g. for a file with conflicts.
    pub conflict: Option<ConflictView>,
    /// Across the top, above the panes, while something is in progress.
    pub banner: Option<Banner>,
    /// At the right of the title bar, e.g. Pull and Push.
    pub toolbar: Vec<ToolbarButton>,
    /// Atop the column at the right: what it shows, e.g. the changes or the history; a
    /// click switches.
    pub tabs: Vec<Button>,
    /// At the left under the facts, its own facts left out, e.g. the changed files; the
    /// rows then go at the right under the tabs, in place of the history.
    pub side: Option<TextSide>,
}

pub struct ToolbarButton {
    pub icon: Icon,
    pub label: String,
    /// In a badge after the label, e.g. commits to push.
    pub count: Option<usize>,
    pub enabled: bool,
    pub action: Action,
}

/// What is in progress and what can be done about it, e.g. a rebase stopped at a
/// conflict.
pub struct Banner {
    /// E.g. `Rebasing`.
    pub title: String,
    /// After the title, e.g. the branch, `onto` and its base.
    pub parts: Vec<BannerPart>,
    /// E.g. `Step 2 of 3`.
    pub step: Option<String>,
    /// Being applied: its id and subject.
    pub commit: Option<(String, String)>,
    /// E.g. `2 conflicts left`.
    pub state: (String, Tone),
    pub buttons: Vec<BannerButton>,
}

pub enum BannerPart {
    Chip(RefChip),
    Word(String),
}

pub struct BannerButton {
    pub label: String,
    /// Danger for one that throws work away, Accent for the one to press next.
    pub tone: Tone,
    pub enabled: bool,
    pub action: Action,
}

/// A file's conflicts: the lines around them and both sides of each, the current one
/// with the keys that resolve it.
pub struct ConflictView {
    pub name: String,
    /// Where it is, e.g. `src/`.
    pub directory: String,
    /// To highlight the lines in.
    pub mode: Option<&'static str>,
    /// Conflicts already resolved here.
    pub resolved: usize,
    /// The current conflict, counted from 1, of those left.
    pub current: usize,
    pub left: usize,
    pub regions: Vec<ConflictRegion>,
    pub previous: Action,
    pub next: Action,
}

pub enum ConflictRegion {
    /// Lines both sides agree on, numbered from `first`; `resolved` says how a conflict
    /// was resolved into them, if one was.
    Text {
        first: usize,
        lines: Vec<String>,
        resolved: Option<String>,
    },
    Conflict(Box<ConflictBlock>),
}

pub struct ConflictBlock {
    /// The one the keys resolve.
    pub current: bool,
    pub ours: ConflictSide,
    pub theirs: ConflictSide,
    pub keep_ours: Action,
    pub keep_theirs: Action,
    pub keep_both: Action,
    /// Visits the file at the conflict.
    pub edit: Action,
}

pub struct ConflictSide {
    pub chip: RefChip,
    /// What the side is, e.g. `upstream`.
    pub role: String,
    pub id: Option<String>,
    pub subject: String,
    /// Numbered from.
    pub first: usize,
    pub lines: Vec<String>,
}

/// Commits, newest first, with the graph of how they descend from each other, like Emacs
/// `magit-log`.
pub struct History {
    /// What the commit messages were searched for, if they were.
    pub search: Option<String>,
    /// The file whose commits alone are shown, if one is; a click shows them all again.
    pub file: Option<Button>,
    /// From the first one scrolled to, more than fit.
    pub commits: Vec<CommitRow>,
    /// Of `commits`, the one the keys act on; kept in sight.
    pub selected: Option<usize>,
    /// Whether newer commits were scrolled past.
    pub scrolled: bool,
    /// Said in place of the commits when there are none.
    pub empty: String,
    /// Moves the first commit shown by so many rows.
    pub scroll: Scroll,
}

pub type Scroll = Rc<dyn Fn(&mut Editor, isize) -> CommandResult>;

pub struct CommitRow {
    pub graph: GraphRow,
    pub refs: Vec<RefChip>,
    pub subject: String,
    pub author: String,
    /// E.g. `2 h ago`.
    pub date: String,
    /// Abbreviated.
    pub id: String,
    /// Run by a click: shows what the commit changed.
    pub open: Action,
}

/// A commit's row of the graph. Lanes are columns counted from the left; colors index
/// the theme's graph palette, the first being the checked out branch's, the second its
/// base's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphRow {
    pub lane: usize,
    pub color: usize,
    pub node: GraphNode,
    /// Lines from the top of the row to its middle.
    pub upper: Vec<GraphEdge>,
    /// Lines from the middle of the row to its bottom.
    pub lower: Vec<GraphEdge>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    pub color: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GraphNode {
    #[default]
    Commit,
    Merge,
    /// Checked out.
    Head,
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

/// Commits on top of a branch, drawn as a short line of dots between their refs, e.g.
/// what a rebase will make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitChain {
    pub base: RefChip,
    pub tip: RefChip,
    /// Dots after the base's, in its color: commits that come in under ours.
    pub incoming: usize,
    /// Dots after those, in the tip's color.
    pub commits: usize,
    /// Under the chain, e.g. what it means for the history.
    pub note: Option<String>,
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
        /// In place of the line counts, e.g. `1 conflict`.
        note: Option<(String, Tone)>,
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
