/// The lines of a buffer shown in a window, like Emacs `window-start` plus height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub first_line: usize,
    pub height: usize,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            first_line: 0,
            height: 1,
        }
    }
}

impl Viewport {
    pub fn last_line(&self) -> usize {
        self.first_line + self.height - 1
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

    pub fn ensure_visible(&mut self, line: usize) {
        if line < self.first_line {
            self.first_line = line;
        } else if line > self.last_line() {
            self.first_line = line + 1 - self.height;
        }
    }

    pub fn center_on(&mut self, line: usize) {
        self.first_line = line.saturating_sub(self.height / 2);
    }

    /// The nearest visible line to `line`.
    pub fn clamp(&self, line: usize) -> usize {
        line.clamp(self.first_line, self.last_line())
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
        };
        viewport.ensure_visible(16);
        assert_eq!(viewport.first_line, 12);
        viewport.ensure_visible(3);
        assert_eq!(viewport.first_line, 3);
        assert!(viewport.scroll_by(-10, 100));
        assert_eq!(viewport.first_line, 0);
        assert!(!viewport.scroll_by(-1, 100));
        assert!(viewport.scroll_by(500, 100));
        assert_eq!(viewport.first_line, 99);
    }
}
