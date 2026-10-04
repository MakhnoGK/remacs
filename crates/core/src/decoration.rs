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
