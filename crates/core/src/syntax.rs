use std::ops::Range;
use std::rc::Rc;

use crate::buffer::Buffer;

/// What kind of text an offset is in, for commands that type differently in code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxContext {
    Code,
    String,
    Comment,
    /// Markup embedded in code, like JSX elements and their text.
    Markup,
}

/// Tells the context of an offset in a buffer, or `None` when it does not know the buffer.
pub type SyntaxProbe = Rc<dyn Fn(&Buffer, usize) -> Option<SyntaxContext>>;

/// Code of another language in a buffer, like CSS in HTML's `<style>`: its mode and where
/// it is, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Embedded {
    pub mode: &'static str,
    pub ranges: Vec<Range<usize>>,
}

impl Embedded {
    /// Whether `offset` is in one of the ranges or at its end.
    pub fn holds(&self, offset: usize) -> bool {
        self.ranges
            .iter()
            .any(|range| range.start <= offset && offset <= range.end)
    }
}

/// Tells the code of other languages in a buffer, or `None` when it cannot tell now.
pub type EmbeddedProbe = Rc<dyn Fn(&Buffer) -> Option<Vec<Embedded>>>;
