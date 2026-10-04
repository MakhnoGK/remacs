use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

use remacs_text::Text;

/// A text with a point (cursor), a mark, a name and a major mode.
pub struct Buffer {
    name: String,
    mode: &'static str,
    text: Text,
    file: Option<PathBuf>,
    point: usize,
    /// Column that vertical motion keeps returning to, like Emacs `temporary-goal-column`.
    goal_column: Option<usize>,
    mark: Option<usize>,
    /// Transient mark mode: the region is highlighted and used only while the mark is active.
    mark_active: bool,
    /// First line shown when the buffer was last in the window, like Emacs `last-window-start`.
    pub window_start: usize,
}

impl Buffer {
    pub fn new(name: impl Into<String>, text: Text, mode: &'static str) -> Self {
        Self {
            name: name.into(),
            mode,
            text,
            file: None,
            point: 0,
            goal_column: None,
            mark: None,
            mark_active: false,
            window_start: 0,
        }
    }

    /// Associates the buffer with `path`, which need not exist until the first save.
    pub fn with_file(mut self, path: PathBuf) -> Self {
        self.file = Some(path);
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn mode(&self) -> &'static str {
        self.mode
    }

    pub(crate) fn set_file(&mut self, path: PathBuf, mode: &'static str) {
        self.file = Some(path);
        self.mode = mode;
    }

    pub fn text(&self) -> &Text {
        &self.text
    }

    pub fn contents(&self, range: Range<usize>) -> String {
        String::from_utf8_lossy(&self.text.bytes_in(range)).into_owned()
    }

    pub fn path(&self) -> Option<&Path> {
        self.file.as_deref()
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

    pub fn char_after(&self, offset: usize) -> Option<char> {
        let end = (offset + 4).min(self.text.len());
        String::from_utf8_lossy(&self.text.bytes_in(offset..end))
            .chars()
            .next()
    }

    pub fn char_before(&self, offset: usize) -> Option<char> {
        (offset > 0).then(|| self.char_after(self.text.previous_char_boundary(offset)))?
    }

    pub fn mark(&self) -> Option<usize> {
        self.mark
    }

    pub fn set_mark(&mut self, offset: usize) {
        self.mark = Some(offset.min(self.text.len()));
    }

    pub fn is_mark_active(&self) -> bool {
        self.mark_active && self.mark.is_some()
    }

    pub fn activate_mark(&mut self) {
        self.mark_active = self.mark.is_some();
    }

    pub fn deactivate_mark(&mut self) {
        self.mark_active = false;
    }

    /// Text between point and mark, whether or not the mark is active.
    pub fn mark_range(&self) -> Option<Range<usize>> {
        self.mark
            .map(|mark| self.point.min(mark)..self.point.max(mark))
    }

    /// The active region, if any.
    pub fn region(&self) -> Option<Range<usize>> {
        self.mark_range().filter(|_| self.mark_active)
    }

    pub fn insert(&mut self, text: &str) {
        let point = self.point;
        self.text.insert(point, text);
        self.mark = self.mark.map(|mark| {
            if mark > point {
                mark + text.len()
            } else {
                mark
            }
        });
        self.mark_active = false;
        self.set_point(point + text.len());
    }

    pub fn delete(&mut self, range: Range<usize>) {
        let shift = |offset: usize| {
            if offset >= range.end {
                offset - range.len()
            } else {
                offset.min(range.start)
            }
        };
        let point = shift(self.point);
        self.mark = self.mark.map(shift);
        self.mark_active = false;
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
        let path = self
            .file
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "buffer has no file"))?;
        self.text.save_as(path)
    }

    /// Undo replaces the whole text, so the mark may no longer fall on a character.
    fn restore_point(&mut self, offset: Option<usize>) -> bool {
        self.mark_active = false;
        self.mark = self.mark.map(|mark| {
            let mark = mark.min(self.text.len());
            if self.text.is_char_boundary(mark) {
                mark
            } else {
                self.text.previous_char_boundary(mark)
            }
        });
        offset.inspect(|&offset| self.set_point(offset)).is_some()
    }
}
