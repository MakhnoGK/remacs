use std::rc::Rc;

use crate::buffer::Buffer;
use crate::decoration::ConsoleColor;
use crate::editor::Editor;
use crate::key::{Key, Modifiers};
use crate::status::Tone;

/// A buffer drawn as a terminal's screen instead of as text: rows of cells in the code
/// font, like Emacs `vterm`.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    /// Top to bottom, as many as fit.
    pub rows: Vec<ScreenRow>,
    pub cursor: Option<ScreenCursor>,
    /// Over the bottom of the rows, e.g. that the program ended.
    pub banner: Option<ScreenBanner>,
    /// In place of the rows while there are none to show, e.g. while the shell starts.
    pub notice: Option<ScreenNotice>,
    /// A mode the keys are in, e.g. copying from the scrollback.
    pub mode: Option<ScreenMode>,
    /// An incremental search going on, in a bar over the bottom of the rows.
    pub search: Option<ScreenSearch>,
    /// More is scrolled past above the top row.
    pub above: bool,
    /// The program just rang the bell.
    pub flash: bool,
    /// The mouse is over a link a click follows: the pointer is a hand.
    pub pointing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenMode {
    /// E.g. `COPY`.
    pub label: &'static str,
    /// E.g. `line 1,214 of 1,382`.
    pub detail: String,
    /// What leaves it.
    pub leave: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenSearch {
    pub input: String,
    /// The match shown, counted from 1, and how many there are.
    pub found: Option<(usize, usize)>,
    pub hints: &'static [(&'static str, &'static str)],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScreenRow {
    /// Left to right, each starting where the one before ends.
    pub runs: Vec<CellRun>,
    /// Beside the row, e.g. how the command started on it ended.
    pub mark: Option<RowMark>,
    /// A prompt starts on the row, which ends the command above it.
    pub prompt: bool,
}

/// Cells next to each other drawn the same way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CellRun {
    pub text: String,
    /// Cells it takes: wide characters take two.
    pub cells: usize,
    pub face: CellFace,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellFace {
    /// The theme's text color when none.
    pub foreground: Option<ConsoleColor>,
    /// The glass when none.
    pub background: Option<ConsoleColor>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    pub highlight: Option<CellHighlight>,
}

/// What a cell is marked as, over its own colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellHighlight {
    Selection,
    SearchMatch,
    SearchCurrent,
    /// Followed by a key or a click, like a `file:line`.
    Link,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenCursor {
    pub row: usize,
    pub column: usize,
    pub shape: CursorShape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
    /// Where the cursor is while the keys are elsewhere.
    Hollow,
}

/// A command's mark beside the row its prompt starts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowMark {
    /// None while it runs.
    pub exit: Option<i32>,
    /// E.g. `exit 101 · 2.8 s · finished 10:41`, shown on hover.
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenBanner {
    pub text: String,
    pub tone: Tone,
    /// Keys and what they do, after the text.
    pub keys: &'static [(&'static str, &'static str)],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenNotice {
    pub title: String,
    pub detail: String,
    pub tone: Tone,
}

/// The room a screen has, in cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScreenSize {
    pub columns: usize,
    pub rows: usize,
}

/// Pixels between a screen's edges and its cells, on each side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScreenPadding {
    pub horizontal: f32,
    pub vertical: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenButton {
    Left,
    Middle,
    Right,
}

/// What the mouse did over a screen's cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenMouse {
    /// Pressed `clicks` times in a row.
    Press {
        button: ScreenButton,
        clicks: usize,
    },
    /// Moved with the button pressed on the screen held.
    Drag,
    Release {
        button: ScreenButton,
    },
    /// Moved with no button held.
    Move,
    /// Left the screen; the cell is none in particular.
    Leave,
    /// The wheel turned by whole rows, up for negative ones.
    Wheel {
        rows: isize,
    },
}

/// The mouse over the cell at `row` and `column` of a screen as last drawn, the cell
/// clamped to the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenPointer {
    pub row: usize,
    pub column: usize,
    pub modifiers: Modifiers,
    pub mouse: ScreenMouse,
}

/// Draws buffers as screens and takes the mouse over them.
pub trait ScreenSource {
    /// Whether this plugin draws `buffer` as a screen; cheap, unlike `screen`.
    fn draws(&self, buffer: &Buffer) -> bool;
    /// `buffer` as a screen of `size`, if this plugin draws it as one; the plugin fits
    /// what it shows to the size.
    fn screen(&self, editor: &Editor, buffer: &Buffer, size: ScreenSize) -> Option<Screen>;
    /// Room around the cells of the screens this plugin draws.
    fn padding(&self, _editor: &Editor) -> ScreenPadding {
        ScreenPadding::default()
    }
    /// The mouse over the buffer named `buffer`; true when the screen changed with it.
    fn mouse(&self, editor: &mut Editor, buffer: &str, pointer: ScreenPointer) -> bool;
}

pub type ScreenProvider = Rc<dyn ScreenSource>;

/// Takes a key before any keymap, e.g. to pass it to a terminal's program; false leaves
/// it to the keymaps.
pub type KeySink = Rc<dyn Fn(&mut Editor, Key) -> bool>;
