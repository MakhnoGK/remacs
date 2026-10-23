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
    /// A function's parameters and local variables, where they are defined and used, like
    /// Emacs `font-lock-variable-name-face` and `-use-face`.
    Variable,
    /// Underlines marking problems, like Emacs `flymake-error`/`-warning`/`-note`.
    Error,
    Warning,
    Info,
    /// The symbol a popup is about, like Emacs `highlight`.
    Highlight,
    /// Matches of a search, like Emacs `lazy-highlight`; the one chosen, like `isearch`.
    SearchMatch,
    SearchCurrent,
    /// A bracket or tag at point and the one it pairs with, like Emacs `show-paren-match`.
    MatchingPair,
    /// What a diff's added or removed line changed against the line it replaces, like
    /// Emacs `diff-refine-added` and `-removed`.
    Added,
    Removed,
    /// Text a program printed, styled by its ANSI escape codes.
    Console(ConsoleStyle),
    /// Followed by a key or a click, like a `file:line` in a program's output.
    Link,
    /// Text naming a color, like `bg-red-500` or `#1e90ff`, marked in it: `0xRRGGBBAA`.
    Swatch(u32),
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

/// Sorted, non-overlapping `spans` cut to each of the sorted `lines`.
pub(crate) fn split_by_line(spans: &[Span], lines: &[Range<usize>]) -> Vec<Vec<Span>> {
    let mut first = 0;
    lines
        .iter()
        .map(|line| {
            first += spans[first..].partition_point(|span| span.range.end <= line.start);
            spans[first..]
                .iter()
                .take_while(|span| span.range.start < line.end)
                .map(|span| Span {
                    range: span.range.start.max(line.start)..span.range.end.min(line.end),
                    face: span.face,
                })
                .filter(|span| !span.range.is_empty())
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_are_cut_to_the_lines_they_cross() {
        let span = |range: Range<usize>| Span {
            range,
            face: Face::String,
        };
        let spans = [span(0..2), span(4..12), span(20..21)];
        let lines = [0..5, 6..10, 11..15, 16..19];
        assert_eq!(
            split_by_line(&spans, &lines),
            [
                vec![span(0..2), span(4..5)],
                vec![span(6..10)],
                vec![span(11..12)],
                vec![],
            ]
        );
    }
}
