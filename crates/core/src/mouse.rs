use std::ops::Range;

use crate::buffer::Buffer;

/// What one press selects, by the clicks in a row it ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    Character,
    Word,
    Line,
}

impl Unit {
    fn of_clicks(clicks: usize) -> Self {
        match clicks {
            0 | 1 => Self::Character,
            2 => Self::Word,
            _ => Self::Line,
        }
    }

    fn range_at(self, buffer: &Buffer, offset: usize) -> Range<usize> {
        match self {
            Self::Character => offset..offset,
            Self::Word => word_at(buffer, offset),
            Self::Line => line_at(buffer, offset),
        }
    }
}

/// A press of the mouse button over the text, followed while the button is held: a
/// click puts point, a double click selects a word, a triple click a line, and dragging
/// extends the region by the same unit.
#[derive(Clone, Debug)]
pub struct MouseSelection {
    /// What the press selected, kept in the region wherever the mouse is dragged.
    anchor: Range<usize>,
    unit: Unit,
}

impl MouseSelection {
    /// With `extend` (shift held), extends the active region, or one from point, to
    /// `offset` instead.
    pub fn press(buffer: &mut Buffer, offset: usize, clicks: usize, extend: bool) -> Self {
        let unit = Unit::of_clicks(clicks);
        if extend {
            let from = buffer
                .mark()
                .filter(|_| buffer.is_mark_active())
                .unwrap_or(buffer.point());
            let selection = Self {
                anchor: from..from,
                unit,
            };
            selection.drag(buffer, offset);
            return selection;
        }
        let anchor = unit.range_at(buffer, offset);
        buffer.set_point(anchor.end);
        if anchor.is_empty() {
            // A plain click keeps the mark, as in Emacs.
            buffer.deactivate_mark();
        } else {
            buffer.set_mark(anchor.start);
            buffer.activate_mark();
        }
        Self { anchor, unit }
    }

    /// Selects from the press to the unit at `offset`.
    pub fn drag(&self, buffer: &mut Buffer, offset: usize) {
        let under = self.unit.range_at(buffer, offset);
        let (mark, point) = if under.start < self.anchor.start {
            (self.anchor.end, under.start)
        } else {
            (self.anchor.start, under.end.max(self.anchor.end))
        };
        buffer.set_mark(mark);
        buffer.set_point(point);
        if mark == point {
            buffer.deactivate_mark();
        } else {
            buffer.activate_mark();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    /// Identifier characters, so a double click takes `snake_case` whole.
    Word,
    Space,
    Punctuation,
}

impl CharClass {
    fn of(character: char) -> Self {
        if character.is_alphanumeric() || character == '_' {
            Self::Word
        } else if character.is_whitespace() {
            Self::Space
        } else {
            Self::Punctuation
        }
    }
}

/// The run of characters of one class around `offset` on its line; at the line's end,
/// the run before it.
fn word_at(buffer: &Buffer, offset: usize) -> Range<usize> {
    let text = buffer.text();
    let line = text
        .line_range(text.line_of(offset))
        .unwrap_or(offset..offset);
    let after = buffer.char_after(offset).filter(|_| offset < line.end);
    let before = buffer.char_before(offset).filter(|_| offset > line.start);
    let Some(class) = after.or(before).map(CharClass::of) else {
        return offset..offset;
    };
    let in_class = |character: Option<char>| character.is_some_and(|c| CharClass::of(c) == class);
    let mut start = offset;
    while start > line.start && in_class(buffer.char_before(start)) {
        start = text.previous_char_boundary(start);
    }
    let mut end = offset;
    while end < line.end && in_class(buffer.char_after(end)) {
        end = text.next_char_boundary(end);
    }
    start..end
}

/// The line at `offset` with its newline.
fn line_at(buffer: &Buffer, offset: usize) -> Range<usize> {
    let text = buffer.text();
    let line = text.line_of(offset);
    let start = text.line_start(line).unwrap_or(text.len());
    let end = text.line_start(line + 1).unwrap_or(text.len());
    start..end
}

#[cfg(test)]
mod tests {
    use remacs_text::Text;

    use super::*;
    use crate::mode::FUNDAMENTAL_MODE;

    const TEXT: &str = "let snake_case = (1);\nsecond line\n";

    fn buffer() -> Buffer {
        Buffer::new("test", Text::from(TEXT), FUNDAMENTAL_MODE)
    }

    fn region(buffer: &Buffer) -> Option<&str> {
        buffer.region().map(|range| &TEXT[range])
    }

    #[test]
    fn a_click_puts_point_and_a_drag_selects_either_way() {
        let mut buffer = buffer();
        let selection = MouseSelection::press(&mut buffer, 4, 1, false);
        assert_eq!((buffer.point(), region(&buffer)), (4, None));
        selection.drag(&mut buffer, 9);
        assert_eq!(region(&buffer), Some("snake"));
        assert_eq!(buffer.point(), 9);
        selection.drag(&mut buffer, 0);
        assert_eq!(region(&buffer), Some("let "));
        selection.drag(&mut buffer, 4);
        assert_eq!(region(&buffer), None);
    }

    #[test]
    fn a_double_click_selects_a_word_and_drags_by_words() {
        let mut buffer = buffer();
        let selection = MouseSelection::press(&mut buffer, 7, 2, false);
        assert_eq!(region(&buffer), Some("snake_case"));
        selection.drag(&mut buffer, 1);
        assert_eq!(region(&buffer), Some("let snake_case"));
        assert_eq!(buffer.point(), 0);
        selection.drag(&mut buffer, 17);
        assert_eq!(region(&buffer), Some("snake_case = ("));
    }

    #[test]
    fn words_are_runs_of_one_class_and_stop_at_the_line() {
        let mut buffer = buffer();
        MouseSelection::press(&mut buffer, 19, 2, false);
        assert_eq!(region(&buffer), Some(");"));
        MouseSelection::press(&mut buffer, 16, 2, false);
        assert_eq!(region(&buffer), Some(" "));
        MouseSelection::press(&mut buffer, 21, 2, false);
        assert_eq!(
            region(&buffer),
            Some(");"),
            "past the end, the run before it"
        );
    }

    #[test]
    fn a_triple_click_selects_lines_with_their_newline() {
        let mut buffer = buffer();
        let selection = MouseSelection::press(&mut buffer, 25, 3, false);
        assert_eq!(region(&buffer), Some("second line\n"));
        selection.drag(&mut buffer, 3);
        assert_eq!(region(&buffer), Some(TEXT));
        assert_eq!(buffer.point(), 0);
    }

    #[test]
    fn a_shift_click_extends_from_the_mark_or_point() {
        let mut buffer = buffer();
        buffer.set_point(4);
        MouseSelection::press(&mut buffer, 9, 1, true);
        assert_eq!(region(&buffer), Some("snake"));
        MouseSelection::press(&mut buffer, 0, 1, true);
        assert_eq!(
            region(&buffer),
            Some("let "),
            "from the active region's mark"
        );
    }
}
