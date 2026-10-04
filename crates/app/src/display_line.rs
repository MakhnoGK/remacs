use std::ops::Range;

const TAB_WIDTH: usize = 4;

/// A line as drawn in a monospace grid: tabs expanded to the next tab stop.
pub struct DisplayLine {
    pub text: String,
    /// Where each source character starts, plus one entry for the end of the line.
    cells: Vec<Cell>,
    /// A space was appended so the end of the line can be drawn on.
    padded: bool,
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
        let mut text = String::with_capacity(source.len());
        let mut cells = Vec::with_capacity(source.len() + 1);
        let mut column = 0;
        for (source_byte, character) in source.char_indices() {
            cells.push(Cell {
                source: source_byte,
                column,
                byte: text.len(),
            });
            if character == '\t' {
                let width = TAB_WIDTH - column % TAB_WIDTH;
                text.extend(std::iter::repeat_n(' ', width));
                column += width;
            } else {
                text.push(character);
                column += 1;
            }
        }
        cells.push(Cell {
            source: source.len(),
            column,
            byte: text.len(),
        });
        Self {
            text,
            cells,
            padded: false,
        }
    }

    /// Byte range of the cell the cursor covers at `char_column`; past the end of the
    /// line it covers the newline, drawn as a space.
    pub fn cursor_cell(&mut self, char_column: usize) -> Range<usize> {
        if char_column >= self.end_column() {
            return self.newline_cell();
        }
        let start = self.cells[char_column].byte;
        let width = self.text[start..].chars().next().map_or(1, char::len_utf8);
        start..start + width
    }

    /// Byte range covering the characters in `char_columns`; a range past the last
    /// character includes the newline.
    pub fn cell_range(&mut self, char_columns: Range<usize>) -> Range<usize> {
        let start = self.cells[char_columns.start.min(self.end_column())].byte;
        let end = if char_columns.end > self.end_column() {
            self.newline_cell().end
        } else {
            self.cells[char_columns.end].byte
        };
        start..end
    }

    /// Byte range of `text` showing the source line's bytes `source`.
    pub fn display_range(&self, source: Range<usize>) -> Range<usize> {
        let byte_at = |offset: usize| {
            let cell = self.cells.partition_point(|cell| cell.source < offset);
            self.cells[cell.min(self.end_column())].byte
        };
        byte_at(source.start)..byte_at(source.end)
    }

    pub fn char_column_at(&self, display_column: usize) -> usize {
        self.cells
            .partition_point(|cell| cell.column <= display_column)
            .saturating_sub(1)
    }

    fn end_column(&self) -> usize {
        self.cells.len() - 1
    }

    fn newline_cell(&mut self) -> Range<usize> {
        let start = self.cells[self.end_column()].byte;
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
    fn cell_ranges_cover_tabs_and_the_newline() {
        let mut line = DisplayLine::new("ж\tx");
        assert_eq!(line.cell_range(1..2), 2..5);
        assert_eq!(line.cell_range(2..4), 5..7);
        assert_eq!(line.cursor_cell(3), 6..7);
        assert_eq!(line.text, "ж   x ");
    }
}
