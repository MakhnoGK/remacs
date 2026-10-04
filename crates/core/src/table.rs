use std::ops::Range;
use std::rc::Rc;

use crate::command::CommandResult;
use crate::completion::Icon;
use crate::editor::Editor;
use crate::popup::Code;
use crate::vc::{FileState, FileStatus};

/// What a click on a table's control does to the window buffer.
pub type Action = Rc<dyn Fn(&mut Editor) -> CommandResult>;

/// A buffer drawn as rows with columns instead of as text, one row per line, like Emacs
/// `tabulated-list-mode`; point's line is the selected row.
pub struct Table {
    /// Where the rows come from, outermost first; the last one is current.
    pub crumbs: Vec<Button>,
    /// Orders the rows can be sorted in.
    pub sort: Vec<Button>,
    pub columns: &'static [Column],
    /// Only the rows of the lines asked for.
    pub rows: Vec<Row>,
    /// Run by a double click on a row, after point moved to it.
    pub activate: &'static str,
    /// Keys and what they do, under the rows.
    pub hints: &'static [(&'static str, &'static str)],
    /// About the selected row, beside the rows when there is room.
    pub preview: Option<Preview>,
    /// Says the rows are being edited, in place of the sort controls.
    pub notice: Option<Notice>,
    /// Edits to the rows not yet applied, beside them in place of the preview.
    pub changes: Option<Changes>,
}

pub struct Notice {
    pub label: &'static str,
    /// Keys that end the editing and what they do.
    pub keys: &'static [(&'static str, &'static str)],
}

pub struct Changes {
    /// E.g. `2 renames`.
    pub summary: String,
    /// What each edited row said and says now.
    pub edits: Vec<(String, String)>,
    /// What applying them also changes in other files.
    pub elsewhere: Elsewhere,
    /// Settings of how they are applied, switched by a click.
    pub options: Vec<Button>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub enum Elsewhere {
    #[default]
    Nothing,
    /// Still being worked out.
    Checking,
    Files(Vec<FileDiff>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiff {
    pub name: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffLine {
    Removed(String),
    Added(String),
}

pub struct Preview {
    pub icon: Icon,
    pub title: String,
    /// What the contents are, e.g. the major mode.
    pub kind: String,
    pub body: PreviewBody,
    /// Labels and values under the body, e.g. the size.
    pub details: Vec<(&'static str, String)>,
    /// What version control, by name, says of it: beside the title and after the details.
    pub version: Option<(&'static str, FileStatus)>,
}

pub enum PreviewBody {
    /// The first lines of a file, drawn numbered.
    Code(Code),
    /// What a directory holds, by name.
    Items(Vec<PreviewItem>),
    /// Why there is nothing to show, e.g. a binary file; empty while it is read.
    Message(String),
}

pub struct PreviewItem {
    pub icon: Icon,
    pub name: String,
    pub face: RowFace,
}

pub struct Button {
    pub label: String,
    pub selected: bool,
    pub action: Action,
}

pub struct Column {
    pub title: &'static str,
    /// Pixels; `None` takes the room the other columns leave.
    pub width: Option<f32>,
    pub style: CellStyle,
}

/// How the cells of a column are drawn; the UI maps styles to theme fonts and colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellStyle {
    /// What the row is, e.g. a file name; takes the row's face.
    Primary,
    /// A number or code, right-aligned in the code font.
    Figure,
    /// Prose, right-aligned in the UI font.
    Label,
    /// Like `Figure`, fainter.
    Faint,
    /// The row's version control state as a letter, colored by it.
    State,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub mark: Option<Mark>,
    pub icon: Option<Icon>,
    /// One per column.
    pub cells: Vec<String>,
    pub face: RowFace,
    /// The first cell is a text field while the rows are edited in place.
    pub field: Option<Field>,
    /// How the row's file differs from the last commit, for a `State` column.
    pub state: Option<FileState>,
}

/// An editable first cell; its text is the row's line, with point's cursor and region.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// What the cell said before, once it was edited.
    pub original: Option<String>,
}

/// What a row is marked for, like the character before a file in Emacs dired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// Chosen for the next command, `*`.
    Marked,
    /// To be deleted, `D`.
    Flagged,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowFace {
    #[default]
    Normal,
    Directory,
    /// Something usually ignored, like a backup file.
    Dim,
}

/// Describes `lines` of the window buffer as a table, if the plugin draws that buffer.
pub type TableProvider = Rc<dyn Fn(&Editor, Range<usize>) -> Option<Table>>;
