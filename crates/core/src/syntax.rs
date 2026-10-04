use std::rc::Rc;

use crate::buffer::Buffer;

/// What kind of text an offset is in, for commands that type differently in code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxContext {
    Code,
    String,
    Comment,
}

/// Tells the context of an offset in a buffer, or `None` when it does not know the buffer.
pub type SyntaxProbe = Rc<dyn Fn(&Buffer, usize) -> Option<SyntaxContext>>;
