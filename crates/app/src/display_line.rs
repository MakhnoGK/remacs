use std::ops::Range;

const TAB_WIDTH: usize = 4;

/// A line, or the part of it in view, as drawn in a monospace grid: tabs expanded to the
/// next tab stop.
pub struct DisplayLine {
    pub text: String,
    /// Where each source character starts, plus one entry for the end of the source.
    cells: Vec<Cell>,
    /// Characters of the line before the source.
    first_char: usize,
    /// The line goes on past the source, out of view.
    pub continues: bool,
    /// A space was appended so the end of the line can be drawn on.
    padded: bool,
}

/// Where a part of a line starts: its characters and display columns before it.
#[derive(Clone, Copy, Default)]
pub struct LineStart {
    pub char: usize,
    pub column: usize,
}

/// The bytes of a line drawn between two display columns.
pub struct LineSlice {
    /// From the line's start.
    pub bytes: Range<usize>,
    pub start: LineStart,
    pub continues: bool,
}

/// The part of the line of `bytes` covering display `columns`, with the character partly
/// shown past their end; reads no further, so a megabyte line costs only what is in view.
pub fn slice_line(bytes: impl IntoIterator<Item = u8>, columns: Range<usize>) -> LineSlice {
    let mut start = None;
    let mut end = None;
    let mut continues = false;
    let (mut char_count, mut column, mut length) = (0, 0, 0);
    for (offset, byte) in bytes.into_iter().enumerate() {
        if !is_utf8_continuation(byte) {
            continues |= column >= columns.end;
            if column > columns.end {
                end = Some(offset);
                break;
            }
            let next = advance(column, byte == b'\t');
            if start.is_none() && next > columns.start {
                start = Some((
                    offset,
                    LineStart {
                        char: char_count,
                        column,
                    },
                ));
            }
            char_count += 1;
            column = next;
        }
        length = offset + 1;
    }
    let end = end.unwrap_or(length);
    let (from, start) = start.unwrap_or((
        end,
        LineStart {
            char: char_count,
            column,
        },
    ));
    LineSlice {
        bytes: from..end,
        start,
        continues,
    }
}

/// The display column after `bytes` from the start of a line.
pub fn display_width(bytes: impl IntoIterator<Item = u8>) -> usize {
    bytes
        .into_iter()
        .filter(|&byte| !is_utf8_continuation(byte))
        .fold(0, |column, byte| advance(column, byte == b'\t'))
}

fn advance(column: usize, is_tab: bool) -> usize {
    if is_tab {
        column + TAB_WIDTH - column % TAB_WIDTH
    } else {
        column + 1
    }
}

fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

#[derive(Clone, Copy)]
struct Cell {
    /// Byte offset in the source line.
    source: usize,
    column: usize,
    /// Byte offset in `text`.
    byte: usize,
}

impl DisplayLine {
    pub fn new(source: &str) -> Self {
        Self::part(source, LineStart::default(), false)
    }

    /// The part of a line from `start`; `continues` past `source`.
    pub fn part(source: &str, start: LineStart, continues: bool) -> Self {
        let mut text = String::with_capacity(source.len());
        let mut cells = Vec::with_capacity(source.len() + 1);
        let mut column = start.column;
        for (source_byte, character) in source.char_indices() {
            cells.push(Cell {
                source: source_byte,
                column,
                byte: text.len(),
            });
            let next = advance(column, character == '\t');
            if character == '\t' {
                text.extend(std::iter::repeat_n(' ', next - column));
            } else {
                text.push(character);
            }
            column = next;
        }
        cells.push(Cell {
            source: source.len(),
            column,
            byte: text.len(),
        });
        Self {
            text,
            cells,
            first_char: start.char,
            continues,
            padded: false,
        }
    }

    /// The line has characters before the source, out of view.
    pub fn has_hidden_start(&self) -> bool {
        self.first_char > 0
    }

    /// The display column `text` starts at.
    pub fn first_column(&self) -> usize {
        self.cells[0].column
    }

    /// Byte range of the cell the cursor covers at the line's `char_column`; past the end
    /// of the line it covers the newline, drawn as a space.
    pub fn cursor_cell(&mut self, char_column: usize) -> Range<usize> {
        let index = self.cell_index(char_column);
        if index >= self.end_cell() {
            return self.newline_cell();
        }
        let start = self.cells[index].byte;
        let width = self.text[start..].chars().next().map_or(1, char::len_utf8);
        start..start + width
    }

    /// Byte range covering the line's characters in `char_columns`; a range past the last
    /// character includes the newline.
    pub fn cell_range(&mut self, char_columns: Range<usize>) -> Range<usize> {
        let start = self.cells[self.cell_index(char_columns.start).min(self.end_cell())].byte;
        let end = self.cell_index(char_columns.end);
        let end = if end > self.end_cell() && !self.continues {
            self.newline_cell().end
        } else {
            self.cells[end.min(self.end_cell())].byte
        };
        start..end
    }

    /// Byte range of `text` showing the source line's bytes `source`.
    pub fn display_range(&self, source: Range<usize>) -> Range<usize> {
        let byte_at = |offset: usize| {
            let cell = self.cells.partition_point(|cell| cell.source < offset);
            self.cells[cell.min(self.end_cell())].byte
        };
        byte_at(source.start)..byte_at(source.end)
    }

    /// The line's character at `display_column`.
    pub fn char_column_at(&self, display_column: usize) -> usize {
        let index = self
            .cells
            .partition_point(|cell| cell.column <= display_column)
            .saturating_sub(1);
        self.first_char + index
    }

    /// Index into `cells` of the line's `char_column`, past the end for one after it.
    fn cell_index(&self, char_column: usize) -> usize {
        char_column.saturating_sub(self.first_char)
    }

    fn end_cell(&self) -> usize {
        self.cells.len() - 1
    }

    fn newline_cell(&mut self) -> Range<usize> {
        let start = self.cells[self.end_cell()].byte;
        if !self.padded {
            self.text.push(' ');
            self.padded = true;
        }
        start..start + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_tabs_to_tab_stops() {
        let line = DisplayLine::new("a\tb");
        assert_eq!(line.text, "a   b");
        assert_eq!(line.char_column_at(2), 1);
        assert_eq!(line.char_column_at(4), 2);
        assert_eq!(line.char_column_at(40), 3);
    }

    #[test]
    fn source_bytes_map_across_tabs_and_wide_chars() {
        let line = DisplayLine::new("ж\tfn x");
        assert_eq!(line.display_range(0..2), 0..2);
        assert_eq!(line.display_range(3..5), 5..7);
        assert_eq!(line.display_range(6..60), 8..9);
    }

    #[test]
    fn cursor_covers_one_cell() {
        let mut line = DisplayLine::new("ж\tx");
        assert_eq!(line.cursor_cell(0), 0..2);
        assert_eq!(line.cursor_cell(1), 2..3);
        assert_eq!(line.cursor_cell(3), 6..7);
        assert_eq!(line.text, "ж   x ");
    }

    #[test]
    fn slices_lines_by_display_columns() {
        let slice = |line: &str, columns| slice_line(line.bytes(), columns);
        let tabbed = slice("a\tbcdefgh", 2..5);
        assert_eq!(tabbed.bytes, 1..4);
        assert_eq!((tabbed.start.char, tabbed.start.column), (1, 1));
        assert!(tabbed.continues);
        let wide = slice("жжжж", 1..2);
        assert_eq!(wide.bytes, 2..6);
        assert!(wide.continues);
        let short = slice("ab", 1..10);
        assert_eq!(short.bytes, 1..2);
        assert!(!short.continues);
        let scrolled_past = slice("ab", 5..10);
        assert_eq!(scrolled_past.bytes, 2..2);
        assert_eq!(scrolled_past.start.char, 2);
        assert_eq!(display_width("ж\tx".bytes()), 5);
    }

    #[test]
    fn parts_of_lines_map_line_columns() {
        let slice = slice_line("a\tbcdefgh".bytes(), 2..5);
        let mut part = DisplayLine::part("\tbc", slice.start, slice.continues);
        assert_eq!(part.text, "   bc");
        assert_eq!(part.first_column(), 1);
        assert_eq!(part.cursor_cell(3), 4..5);
        assert_eq!(part.char_column_at(5), 3);
        assert_eq!(part.cell_range(2..20), 3..5);
        assert_eq!(part.text, "   bc");
    }

    #[test]
    fn cell_ranges_cover_tabs_and_the_newline() {
        let mut line = DisplayLine::new("ж\tx");
        assert_eq!(line.cell_range(1..2), 2..5);
        assert_eq!(line.cell_range(2..4), 5..7);
        assert_eq!(line.cursor_cell(3), 6..7);
        assert_eq!(line.text, "ж   x ");
    }
}
