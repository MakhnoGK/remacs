use std::ops::Range;
use std::rc::Rc;

use crate::buffer::Buffer;

/// How a span of text is drawn; the UI maps faces to theme colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Keyword,
    Type,
    Function,
    Constant,
    Builtin,
    String,
    Comment,
    Docstring,
    /// Underlines marking problems, like Emacs `flymake-error`/`-warning`/`-note`.
    Error,
    Warning,
    Info,
    /// The symbol a popup is about, like Emacs `highlight`.
    Highlight,
    /// Matches of a search, like Emacs `lazy-highlight`; the one chosen, like `isearch`.
    SearchMatch,
    SearchCurrent,
    /// Text a program printed, styled by its ANSI escape codes.
    Console(ConsoleStyle),
    /// Followed by a key or a click, like a `file:line` in a program's output.
    Link,
}

/// How a program asked for its text to be drawn, with SGR escape codes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConsoleStyle {
    pub color: Option<ConsoleColor>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleColor {
    /// 0 to 15 are the theme's, the rest xterm's color cube and grays.
    Indexed(u8),
    /// `0xRRGGBB`.
    Rgb(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Byte offsets in the buffer.
    pub range: Range<usize>,
    pub face: Face,
}

/// Faces for the bytes of `range` of a buffer, sorted and non-overlapping; like Emacs
/// font-lock, asked only for what is on screen.
pub type Decorator = Rc<dyn Fn(&Buffer, Range<usize>) -> Vec<Span>>;
