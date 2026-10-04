use std::ops::Range;
use std::rc::Rc;

use crate::buffer::Buffer;
use crate::command::CommandResult;
use crate::completion::Icon;
use crate::editor::Editor;

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub icon: Option<Icon>,
    /// One per column.
    pub cells: Vec<String>,
    pub face: RowFace,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowFace {
    #[default]
    Normal,
    Directory,
    /// Something usually ignored, like a backup file.
    Dim,
}

/// Describes `lines` of a buffer as a table, if the plugin draws that buffer.
pub type TableProvider = Rc<dyn Fn(&Buffer, Range<usize>) -> Option<Table>>;
