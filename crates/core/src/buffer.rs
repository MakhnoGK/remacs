use std::any::{Any, TypeId};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

use remacs_text::Text;

use crate::command::CommandResult;

/// A text with a point (cursor), a mark, a name and a major mode.
pub struct Buffer {
    name: String,
    mode: &'static str,
    text: Text,
    file: Option<PathBuf>,
    directory: Option<PathBuf>,
    read_only: bool,
    point: usize,
    /// Column that vertical motion keeps returning to, like Emacs `temporary-goal-column`.
    goal_column: Option<usize>,
    mark: Option<usize>,
    /// Transient mark mode: the region is highlighted and used only while the mark is active.
    mark_active: bool,
    /// First line shown when the buffer was last in the window, like Emacs `last-window-start`.
    pub window_start: usize,
    locals: HashMap<TypeId, Box<dyn Any>>,
}

impl Buffer {
    pub fn new(name: impl Into<String>, text: Text, mode: &'static str) -> Self {
        Self {
            name: name.into(),
            mode,
            text,
            file: None,
            directory: None,
            read_only: false,
            point: 0,
            goal_column: None,
            mark: None,
            mark_active: false,
            window_start: 0,
            locals: HashMap::new(),
        }
    }

    /// State a plugin keeps for this buffer, like an Emacs buffer-local variable; it is
    /// dropped with the buffer.
    pub fn local<T: 'static>(&self) -> Option<&T> {
        self.locals.get(&TypeId::of::<T>())?.downcast_ref()
    }

    pub fn local_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.locals.get_mut(&TypeId::of::<T>())?.downcast_mut()
    }

    pub fn set_local<T: 'static>(&mut self, value: T) -> &mut T {
        let local = self
            .locals
            .entry(TypeId::of::<T>())
            .insert_entry(Box::new(value))
            .into_mut();
        local
            .downcast_mut()
            .expect("locals are keyed by their type")
    }

    pub fn remove_local<T: 'static>(&mut self) {
        self.locals.remove(&TypeId::of::<T>());
    }

    /// Associates the buffer with `path`, which need not exist until the first save.
    pub fn with_file(mut self, path: PathBuf) -> Self {
        self.file = Some(path);
        self
    }

    /// Sets the directory the buffer is about, for a buffer that visits no file.
    pub fn with_directory(mut self, directory: PathBuf) -> Self {
        self.directory = Some(directory);
        self
    }

    /// Where file names typed in this buffer are relative to, like Emacs
    /// `default-directory`: its own directory, else its file's.
    pub fn default_directory(&self) -> Option<&Path> {
        self.directory
            .as_deref()
            .or_else(|| self.file.as_deref()?.parent())
    }

    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Lets commands edit the buffer, or stops them, like Emacs `read-only-mode`.
    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    /// Fails in a read-only buffer, like Emacs `barf-if-buffer-read-only`.
    pub fn ensure_writable(&self) -> CommandResult {
        if self.read_only {
            return Err(format!("Buffer is read-only: {}", self.name).into());
        }
        Ok(())
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

    /// Swaps in a new text, read-only or not, dropping undo history and the mark; for a
    /// buffer that a plugin regenerates, like a directory listing.
    pub fn replace_text(&mut self, text: Text) {
        self.text = text;
        self.mark = None;
        self.mark_active = false;
        self.set_point(self.point);
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

    /// Makes `edits`, all in offsets of the current text, last first so the others stay
    /// valid; inserts at one offset keep their order and point stays on its character.
    pub fn apply_edits(&mut self, edits: Vec<(Range<usize>, String)>) {
        let mut edits: Vec<_> = edits.into_iter().enumerate().collect();
        edits.sort_by_key(|(index, (range, _))| Reverse((range.start, *index)));
        let mut point = self.point;
        let len = self.text.len();
        for (_, (range, text)) in edits {
            let range = range.start.min(len)..range.end.min(len);
            point = shift(point, &range, text.len());
            self.delete(range.clone());
            self.set_point(range.start);
            self.insert(&text);
        }
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

/// Edits to one file, in offsets of the text of the buffer visiting it, else of the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEdits {
    pub path: PathBuf,
    /// Replacements that do not overlap; inserts at one offset keep their order.
    pub edits: Vec<(Range<usize>, String)>,
}

/// Where `offset` ends up when `range` is replaced with `inserted` bytes: text inserted
/// at it goes after it, like an Emacs marker.
fn shift(offset: usize, range: &Range<usize>, inserted: usize) -> usize {
    if offset <= range.start {
        offset
    } else if offset >= range.end {
        offset - range.len() + inserted
    } else {
        range.start
    }
}
