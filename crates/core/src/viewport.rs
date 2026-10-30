use std::ops::RangeInclusive;

use crate::folds::ShownLines;

/// The lines of a buffer shown in a window, like Emacs `window-start` plus height.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Viewport {
    pub first_line: usize,
    /// 0 until the window is first laid out.
    pub height: usize,
    /// Lines at the top and bottom that point stays out of while there are more lines
    /// beyond them, like Emacs `scroll-margin`: the window fades them.
    pub margins: (usize, usize),
    /// The display column at the left edge, like Emacs `window-hscroll`.
    pub first_column: usize,
    /// Columns that fit across; 0 until the window is first laid out.
    pub width: usize,
    /// Columns at the left and right that point stays out of while scrolled sideways,
    /// like Emacs `hscroll-margin`: the window fades text past them.
    pub column_margin: usize,
}

/// Its height, margins and moves count rows: the lines `ShownLines` shows, folds' hidden
/// lines left out; `first_line` is a line.
impl Viewport {
    pub fn last_line(&self, shown: &ShownLines) -> usize {
        shown.line_by(self.first_line, self.height.max(1) as isize - 1)
    }

    pub fn contains(&self, line: usize, shown: &ShownLines) -> bool {
        let first = shown.row_of(self.first_line);
        (first..first + self.height.max(1)).contains(&shown.row_of(line))
    }

    pub fn set_height(&mut self, height: usize) {
        self.height = height.max(1);
    }

    /// Scrolls by `rows`; returns whether the viewport moved.
    pub fn scroll_by(&mut self, rows: isize, shown: &ShownLines) -> bool {
        let first_line = self.first_line;
        self.in_rows(shown, |viewport, row_count| viewport.scroll_rows(rows, row_count));
        first_line != self.first_line
    }

    /// Does nothing before the first layout: a window of unknown height would scroll
    /// lines that will fit into view out of it.
    pub fn ensure_visible(&mut self, line: usize, shown: &ShownLines) {
        let row = shown.row_of(line);
        self.in_rows(shown, |viewport, row_count| viewport.ensure_row_visible(row, row_count));
    }

    pub fn center_on(&mut self, line: usize, shown: &ShownLines) {
        let row = shown.row_of(line);
        self.in_rows(shown, |viewport, _| {
            viewport.first_line = row.saturating_sub(viewport.height / 2)
        });
    }

    /// Scrolls `line` to the top, just below the top margin.
    pub fn put_at_top(&mut self, line: usize, shown: &ShownLines) {
        let row = shown.row_of(line);
        self.in_rows(shown, |viewport, _| {
            viewport.first_line = row.saturating_sub(viewport.fitting_margins().0);
        });
    }

    /// Scrolls `line` to the bottom, just above the bottom margin.
    pub fn put_at_bottom(&mut self, line: usize, shown: &ShownLines) {
        let row = shown.row_of(line);
        self.in_rows(shown, |viewport, _| {
            viewport.first_line =
                (row + viewport.fitting_margins().1 + 1).saturating_sub(viewport.height.max(1));
        });
    }

    /// The nearest line to `line` that point can be on without scrolling.
    pub fn clamp(&self, line: usize, shown: &ShownLines) -> usize {
        let row = shown.row_of(line);
        let mut rows = *self;
        let row = rows.in_rows(shown, |viewport, row_count| {
            let clear = viewport.clear_rows(row_count);
            row.clamp(*clear.start(), *clear.end())
        });
        shown.line_at(row)
    }

    /// Runs `moves` on the viewport counted in rows, its first line a row, with the count
    /// of rows.
    fn in_rows<T>(&mut self, shown: &ShownLines, moves: impl FnOnce(&mut Self, usize) -> T) -> T {
        let mut rows = Self { first_line: shown.row_of(self.first_line), ..*self };
        let result = moves(&mut rows, shown.rows());
        self.first_line = shown.line_at(rows.first_line);
        result
    }

    fn scroll_rows(&mut self, rows: isize, row_count: usize) {
        let last_start = row_count.saturating_sub(1);
        self.first_line = self.first_line.saturating_add_signed(rows).min(last_start);
    }

    fn ensure_row_visible(&mut self, line: usize, line_count: usize) {
        if self.height == 0 {
            return;
        }
        let (top_margin, bottom_margin) = self.fitting_margins();
        let top = line.saturating_sub(top_margin);
        let bottom = (line + bottom_margin).min(line_count.saturating_sub(1)).max(line);
        if top < self.first_line {
            self.first_line = top;
        } else if bottom > self.last_row() {
            self.first_line = bottom + 1 - self.height;
        }
    }

    fn last_row(&self) -> usize {
        self.first_line + self.height.max(1) - 1
    }

    /// Scrolls sideways only as far as keeps display `column` out of the side margins,
    /// unlike Emacs, which recenters; nothing before the first layout.
    pub fn ensure_column_visible(&mut self, column: usize) {
        if self.width == 0 {
            return;
        }
        let margin = if 2 * self.column_margin < self.width { self.column_margin } else { 0 };
        let left = column.saturating_sub(margin);
        let right = column + margin;
        if left < self.first_column {
            self.first_column = left;
        } else if right >= self.first_column + self.width {
            self.first_column = right + 1 - self.width;
        }
    }

    /// The rows out of the margins at edges with more rows beyond.
    fn clear_rows(&self, row_count: usize) -> RangeInclusive<usize> {
        let (top_margin, bottom_margin) = self.fitting_margins();
        let top = if self.first_line > 0 { top_margin } else { 0 };
        let bottom = if self.last_row() + 1 < row_count { bottom_margin } else { 0 };
        self.first_line + top..=self.last_row() - bottom
    }

    /// The margins, or none if they would leave no line clear.
    fn fitting_margins(&self) -> (usize, usize) {
        let (top, bottom) = self.margins;
        if top + bottom < self.height.max(1) { self.margins } else { (0, 0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(first_line: usize, height: usize) -> Viewport {
        Viewport { first_line, height, ..Viewport::default() }
    }

    fn columns(width: usize, column_margin: usize) -> Viewport {
        Viewport { width, column_margin, ..Viewport::default() }
    }

    #[test]
    fn scrolls_only_as_far_as_needed() {
        let mut viewport = rows(10, 5);
        let all = &ShownLines::all(100);
        viewport.ensure_visible(16, all);
        assert_eq!(viewport.first_line, 12);
        viewport.ensure_visible(3, all);
        assert_eq!(viewport.first_line, 3);
        assert!(viewport.scroll_by(-10, all));
        assert_eq!(viewport.first_line, 0);
        assert!(!viewport.scroll_by(-1, all));
        assert!(viewport.scroll_by(500, all));
        assert_eq!(viewport.first_line, 99);
    }

    #[test]
    fn counts_rows_of_the_lines_shown() {
        let mut viewport = rows(0, 5);
        // Lines 2 to 51 folded into line 1.
        let shown = &ShownLines::new(100, Some(2..52));
        assert_eq!(viewport.last_line(shown), 54);
        assert!(viewport.contains(54, shown) && !viewport.contains(55, shown));
        viewport.ensure_visible(55, shown);
        assert_eq!(viewport.first_line, 1);
        assert!(viewport.scroll_by(1, shown));
        assert_eq!(viewport.first_line, 52, "the hidden lines are scrolled past at once");
        viewport.center_on(60, shown);
        assert_eq!(viewport.first_line, 58);
        assert_eq!(viewport.clamp(0, shown), 58);
    }

    #[test]
    fn keeps_point_out_of_margins_with_lines_beyond() {
        let mut viewport = rows(10, 5);
        viewport.margins = (1, 1);
        let all = &ShownLines::all(100);
        viewport.ensure_visible(14, all);
        assert_eq!(viewport.first_line, 11);
        viewport.ensure_visible(11, all);
        assert_eq!(viewport.first_line, 10);
        assert_eq!(viewport.clamp(20, all), 13);
        assert_eq!(viewport.clamp(0, all), 11);
        viewport.ensure_visible(0, all);
        assert_eq!(viewport.first_line, 0);
        assert_eq!(viewport.clamp(0, all), 0);
        viewport.ensure_visible(99, all);
        assert_eq!(viewport.first_line, 95);
        assert_eq!(viewport.clamp(99, all), 99);
    }

    #[test]
    fn scrolls_sideways_before_point_reaches_an_edge() {
        let mut viewport = columns(20, 7);
        viewport.ensure_column_visible(12);
        assert_eq!(viewport.first_column, 0);
        viewport.ensure_column_visible(13);
        assert_eq!(viewport.first_column, 1);
        viewport.ensure_column_visible(100);
        assert_eq!(viewport.first_column, 88);
        viewport.ensure_column_visible(95);
        assert_eq!(viewport.first_column, 88);
        viewport.ensure_column_visible(94);
        assert_eq!(viewport.first_column, 87);
        viewport.ensure_column_visible(3);
        assert_eq!(viewport.first_column, 0);
    }

    #[test]
    fn drops_side_margins_that_leave_no_column_clear() {
        let mut viewport = columns(10, 7);
        viewport.ensure_column_visible(10);
        assert_eq!(viewport.first_column, 1);
    }
}
