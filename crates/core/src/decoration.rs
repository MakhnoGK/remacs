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
