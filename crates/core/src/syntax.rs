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

/// Tells the mode of code of another language at an offset in a buffer, like CSS in
/// HTML's `<style>`; `None` where the buffer's own language is.
pub type LanguageProbe = Rc<dyn Fn(&Buffer, usize) -> Option<&'static str>>;
