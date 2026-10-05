use std::ops::RangeInclusive;

/// The lines of a buffer shown in a window, like Emacs `window-start` plus height.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Viewport {
    pub first_line: usize,
    /// 0 until the window is first laid out.
    pub height: usize,
    /// Lines at the top and bottom that point stays out of while there are more lines
    /// beyond them, like Emacs `scroll-margin`: the window fades them.
    pub margins: (usize, usize),
}

impl Viewport {
    pub fn last_line(&self) -> usize {
        self.first_line + self.height.max(1) - 1
    }

    pub fn contains(&self, line: usize) -> bool {
        (self.first_line..=self.last_line()).contains(&line)
    }

    pub fn set_height(&mut self, height: usize) {
        self.height = height.max(1);
    }

    /// Returns whether the viewport moved.
    pub fn scroll_by(&mut self, lines: isize, line_count: usize) -> bool {
        let last_start = line_count.saturating_sub(1);
        let first_line = self.first_line.saturating_add_signed(lines).min(last_start);
        let moved = first_line != self.first_line;
        self.first_line = first_line;
        moved
    }

    /// Does nothing before the first layout: a window of unknown height would scroll
    /// lines that will fit into view out of it.
    pub fn ensure_visible(&mut self, line: usize, line_count: usize) {
        if self.height == 0 {
            return;
        }
        let (top_margin, bottom_margin) = self.fitting_margins();
        let top = line.saturating_sub(top_margin);
        let bottom = (line + bottom_margin)
            .min(line_count.saturating_sub(1))
            .max(line);
        if top < self.first_line {
            self.first_line = top;
        } else if bottom > self.last_line() {
            self.first_line = bottom + 1 - self.height;
        }
    }

    pub fn center_on(&mut self, line: usize) {
        self.first_line = line.saturating_sub(self.height / 2);
    }

    /// The nearest line to `line` that point can be on without scrolling.
    pub fn clamp(&self, line: usize, line_count: usize) -> usize {
        let lines = self.clear_lines(line_count);
        line.clamp(*lines.start(), *lines.end())
    }

    /// The lines out of the margins at edges with more lines beyond.
    fn clear_lines(&self, line_count: usize) -> RangeInclusive<usize> {
        let (top_margin, bottom_margin) = self.fitting_margins();
        let top = if self.first_line > 0 { top_margin } else { 0 };
        let bottom = if self.last_line() + 1 < line_count {
            bottom_margin
        } else {
            0
        };
        self.first_line + top..=self.last_line() - bottom
    }

    /// The margins, or none if they would leave no line clear.
    fn fitting_margins(&self) -> (usize, usize) {
        let (top, bottom) = self.margins;
        if top + bottom < self.height.max(1) {
            self.margins
        } else {
            (0, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolls_only_as_far_as_needed() {
        let mut viewport = Viewport {
            first_line: 10,
            height: 5,
            ..Viewport::default()
        };
        viewport.ensure_visible(16, 100);
        assert_eq!(viewport.first_line, 12);
        viewport.ensure_visible(3, 100);
        assert_eq!(viewport.first_line, 3);
        assert!(viewport.scroll_by(-10, 100));
        assert_eq!(viewport.first_line, 0);
        assert!(!viewport.scroll_by(-1, 100));
        assert!(viewport.scroll_by(500, 100));
        assert_eq!(viewport.first_line, 99);
    }

    #[test]
    fn keeps_point_out_of_margins_with_lines_beyond() {
        let mut viewport = Viewport {
            first_line: 10,
            height: 5,
            margins: (1, 1),
        };
        viewport.ensure_visible(14, 100);
        assert_eq!(viewport.first_line, 11);
        viewport.ensure_visible(11, 100);
        assert_eq!(viewport.first_line, 10);
        assert_eq!(viewport.clamp(20, 100), 13);
        assert_eq!(viewport.clamp(0, 100), 11);
        viewport.ensure_visible(0, 100);
        assert_eq!(viewport.first_line, 0);
        assert_eq!(viewport.clamp(0, 100), 0);
        viewport.ensure_visible(99, 100);
        assert_eq!(viewport.first_line, 95);
        assert_eq!(viewport.clamp(99, 100), 99);
    }
}
