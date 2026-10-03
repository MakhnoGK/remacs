use std::ops::Range;

const TAB_WIDTH: usize = 4;

/// A line as drawn in a monospace grid: tabs expanded to the next tab stop.
pub struct DisplayLine {
    pub text: String,
    /// Display column and byte offset in `text` where each source character starts,
    /// plus one entry for the end of the line.
    cells: Vec<Cell>,
}

#[derive(Clone, Copy)]
struct Cell {
    column: usize,
    byte: usize,
}

impl DisplayLine {
    pub fn new(source: &str) -> Self {
        let mut text = String::with_capacity(source.len());
        let mut cells = Vec::with_capacity(source.len() + 1);
        let mut column = 0;
        for character in source.chars() {
            cells.push(Cell {
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
            column,
            byte: text.len(),
        });
        Self { text, cells }
    }

    /// Byte range of the cell the cursor covers at `char_column`; past the end of the
    /// line a space is appended so the cursor has a cell to draw on.
    pub fn cursor_cell(&mut self, char_column: usize) -> Range<usize> {
        let start = self.cells[char_column.min(self.cells.len() - 1)].byte;
        if start == self.text.len() {
            self.text.push(' ');
        }
        let width = self.text[start..].chars().next().map_or(1, char::len_utf8);
        start..start + width
    }

    pub fn char_column_at(&self, display_column: usize) -> usize {
        self.cells
            .partition_point(|cell| cell.column <= display_column)
            .saturating_sub(1)
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
    fn cursor_covers_one_cell() {
        let mut line = DisplayLine::new("ж\tx");
        assert_eq!(line.cursor_cell(0), 0..2);
        assert_eq!(line.cursor_cell(1), 2..3);
        assert_eq!(line.cursor_cell(3), 6..7);
        assert_eq!(line.text, "ж   x ");
    }
}
