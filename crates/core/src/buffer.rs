use std::io;
use std::ops::Range;
use std::path::Path;

use remacs_text::Text;

/// A text with a point (cursor), a name and a major mode.
pub struct Buffer {
    name: String,
    mode: &'static str,
    text: Text,
    point: usize,
    /// Column that vertical motion keeps returning to, like Emacs `temporary-goal-column`.
    goal_column: Option<usize>,
}

impl Buffer {
    pub fn new(name: impl Into<String>, text: Text, mode: &'static str) -> Self {
        Self {
            name: name.into(),
            mode,
            text,
            point: 0,
            goal_column: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn mode(&self) -> &'static str {
        self.mode
    }

    pub fn text(&self) -> &Text {
        &self.text
    }

    pub fn path(&self) -> Option<&Path> {
        self.text.path()
    }

    pub fn is_modified(&self) -> bool {
        self.text.is_modified()
    }

    pub fn point(&self) -> usize {
        self.point
    }

    pub fn point_line(&self) -> usize {
        self.text.line_of(self.point)
    }

    pub fn point_column(&self) -> usize {
        self.text.char_column(self.point)
    }

    pub fn set_point(&mut self, offset: usize) {
        self.point = offset.min(self.text.len());
        self.goal_column = None;
    }

    /// Moves to `line`, keeping the column vertical motion started from.
    pub fn move_to_line(&mut self, line: usize) {
        let goal_column = self.goal_column.unwrap_or_else(|| self.point_column());
        self.point = self.text.offset_at_char_column(line, goal_column);
        self.goal_column = Some(goal_column);
    }

    pub fn insert(&mut self, text: &str) {
        self.text.insert(self.point, text);
        self.set_point(self.point + text.len());
    }

    pub fn delete(&mut self, range: Range<usize>) {
        let point = if self.point >= range.end {
            self.point - range.len()
        } else {
            self.point.min(range.start)
        };
        self.text.delete(range);
        self.set_point(point);
    }

    pub fn undo(&mut self) -> bool {
        let offset = self.text.undo();
        self.restore_point(offset)
    }

    pub fn redo(&mut self) -> bool {
        let offset = self.text.redo();
        self.restore_point(offset)
    }

    pub fn add_undo_boundary(&mut self) {
        self.text.add_undo_boundary();
    }

    pub fn save(&mut self) -> io::Result<()> {
        self.text.save()
    }

    fn restore_point(&mut self, offset: Option<usize>) -> bool {
        offset.inspect(|&offset| self.set_point(offset)).is_some()
    }
}
