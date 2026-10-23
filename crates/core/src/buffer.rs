use std::any::{Any, TypeId};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::io;
use std::iter;
use std::mem;
use std::ops::{Range, RangeInclusive};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use remacs_text::{Text, UTF_8};

use crate::command::CommandResult;
use crate::kill_ring::KillRing;
use crate::window::WindowId;

/// Like Emacs `mark-ring-max`.
const MARK_RING_MAX: usize = 16;

/// Names a buffer for good, as its name changes with renames and the files it visits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BufferId(u64);

/// A text with a point (cursor), a mark, a name and a major mode.
pub struct Buffer {
    id: BufferId,
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
    /// Earlier marks, the latest last, like Emacs `mark-ring`.
    mark_ring: Vec<usize>,
    /// Transient mark mode: the region is highlighted and used only while the mark is active.
    mark_active: bool,
    /// See [`Buffer::is_rectangle`].
    rectangle: bool,
    /// First line shown when the buffer was last in the window, like Emacs `last-window-start`.
    pub window_start: usize,
    /// Points of the windows showing the buffer but the selected one, which edits move
    /// like the mark.
    window_points: Vec<(WindowId, usize)>,
    /// More places commands run at, ordered by point, none at point.
    cursors: Vec<Cursor>,
    /// Where the cursors were at the last undo boundary, before each undo group since, and
    /// before each undo, so that undo and redo put them back with the text.
    cursors_at_boundary: Vec<CursorPlace>,
    cursors_before_groups: Vec<Vec<CursorPlace>>,
    cursors_undone: Vec<Vec<CursorPlace>>,
    locals: HashMap<TypeId, Box<dyn Any>>,
}

/// A cursor's point, mark and whether the mark is active, as the undo history keeps it.
type CursorPlace = (usize, Option<usize>, bool);

/// A point and mark of their own that commands marked to run at every cursor also run
/// at, like Emacs multiple-cursors' fake cursors; edits move them as they move the mark.
#[derive(Clone, Default)]
pub struct Cursor {
    pub point: usize,
    pub mark: Option<usize>,
    pub mark_active: bool,
    goal_column: Option<usize>,
    /// What this cursor's kills went to, starting as the kill ring was when it was made.
    pub(crate) kill_ring: KillRing,
}

impl Cursor {
    pub fn region(&self) -> Option<Range<usize>> {
        let mark = self.mark.filter(|_| self.mark_active)?;
        Some(self.point.min(mark)..self.point.max(mark))
    }
}

impl Buffer {
    pub fn new(name: impl Into<String>, text: Text, mode: &'static str) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            id: BufferId(NEXT_ID.fetch_add(1, Ordering::Relaxed)),
            name: name.into(),
            mode,
            text,
            file: None,
            directory: None,
            read_only: false,
            point: 0,
            goal_column: None,
            mark: None,
            mark_ring: Vec::new(),
            mark_active: false,
            rectangle: false,
            window_start: 0,
            window_points: Vec::new(),
            cursors: Vec::new(),
            cursors_at_boundary: Vec::new(),
            cursors_before_groups: Vec::new(),
            cursors_undone: Vec::new(),
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

    pub fn id(&self) -> BufferId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Keeps point as `window`'s, for when that window shows the buffer unselected.
    pub(crate) fn park_point(&mut self, window: WindowId) {
        self.forget_window(window);
        self.window_points.push((window, self.point));
    }

    /// Makes point `window`'s parked one again, if it has one.
    pub(crate) fn unpark_point(&mut self, window: WindowId) {
        if let Some(index) = self.window_points.iter().position(|(id, _)| *id == window) {
            self.point = self.window_points.remove(index).1;
        }
    }

    pub(crate) fn forget_window(&mut self, window: WindowId) {
        self.window_points.retain(|(id, _)| *id != window);
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
        self.mark_ring.clear();
        self.cursors.clear();
        self.cursors_at_boundary.clear();
        self.cursors_before_groups.clear();
        self.cursors_undone.clear();
        self.mark_active = false;
        self.set_point(self.point);
        for (_, point) in &mut self.window_points {
            *point = on_character(&self.text, *point);
        }
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

    /// Sets the mark at point, the one before onto the mark ring, like Emacs `push-mark`:
    /// commands that move far call it, so that `pop_mark` comes back.
    pub fn push_mark(&mut self) {
        if let Some(mark) = self.mark.filter(|&mark| mark != self.point) {
            if self.mark_ring.len() == MARK_RING_MAX {
                self.mark_ring.remove(0);
            }
            self.mark_ring.push(mark);
        }
        self.mark = Some(self.point);
    }

    /// Moves point to the mark and the mark to the latest of the mark ring, the mark that
    /// was there going to its oldest end, like Emacs `pop-to-mark-command`; `false` when
    /// there is no mark.
    pub fn pop_mark(&mut self) -> bool {
        let Some(mark) = self.mark else {
            return false;
        };
        self.set_point(mark);
        self.mark_ring.insert(0, mark);
        self.mark = self.mark_ring.pop();
        self.mark_active = false;
        true
    }

    pub fn is_mark_active(&self) -> bool {
        self.mark_active && self.mark.is_some()
    }

    /// A region activated anew is not a rectangle until it is made one.
    pub fn activate_mark(&mut self) {
        if !self.mark_active {
            self.rectangle = false;
        }
        self.mark_active = self.mark.is_some();
    }

    /// Whether the active region is a rectangle: the columns between point and mark on
    /// the lines between them, like Emacs `rectangle-mark-mode`.
    pub fn is_rectangle(&self) -> bool {
        self.rectangle && self.is_mark_active()
    }

    /// The lines and character columns between point and mark, whether or not the mark is
    /// active.
    // shortcut: columns count characters, so a tab counts as one; use display columns if
    // rectangles over tabs come up.
    pub fn rectangle(&self) -> Option<(RangeInclusive<usize>, Range<usize>)> {
        let mark = self.mark?;
        let (point_line, mark_line) = (self.point_line(), self.text.line_of(mark));
        let (point_column, mark_column) = (self.point_column(), self.text.char_column(mark));
        Some((
            point_line.min(mark_line)..=point_line.max(mark_line),
            point_column.min(mark_column)..point_column.max(mark_column),
        ))
    }

    /// Makes the active region a rectangle, or a run of text again.
    pub fn set_rectangle(&mut self, rectangle: bool) {
        self.rectangle = rectangle;
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

    pub fn cursors(&self) -> &[Cursor] {
        &self.cursors
    }

    /// Adds a cursor at each point, its region from the mark when given, but where one is;
    /// each starts with a copy of `kill_ring`.
    pub fn add_cursors(
        &mut self,
        places: impl IntoIterator<Item = (usize, Option<usize>)>,
        kill_ring: &KillRing,
    ) {
        let len = self.text.len();
        self.cursors
            .extend(places.into_iter().map(|(point, mark)| Cursor {
                point: point.min(len),
                mark: mark.map(|mark| mark.min(len)),
                mark_active: mark.is_some(),
                goal_column: None,
                kill_ring: kill_ring.clone(),
            }));
        self.merge_cursors();
    }

    pub fn remove_cursor(&mut self, index: usize) -> Cursor {
        self.cursors.remove(index)
    }

    pub fn clear_cursors(&mut self) {
        self.cursors.clear();
    }

    /// Deactivates the mark of every cursor.
    pub fn deactivate_cursor_marks(&mut self) {
        for cursor in &mut self.cursors {
            cursor.mark_active = false;
        }
    }

    /// Trades point and mark with cursor `index`, so that a command runs there; trading
    /// again trades back.
    pub(crate) fn swap_cursor(&mut self, index: usize) {
        let cursor = &mut self.cursors[index];
        mem::swap(&mut self.point, &mut cursor.point);
        mem::swap(&mut self.mark, &mut cursor.mark);
        mem::swap(&mut self.mark_active, &mut cursor.mark_active);
        mem::swap(&mut self.goal_column, &mut cursor.goal_column);
    }

    pub(crate) fn cursor_kill_ring(&mut self, index: usize) -> &mut KillRing {
        &mut self.cursors[index].kill_ring
    }

    /// Orders the cursors by point and drops those that ran into point or each other.
    pub(crate) fn merge_cursors(&mut self) {
        let point = self.point;
        self.cursors.retain(|cursor| cursor.point != point);
        self.cursors.sort_by_key(|cursor| cursor.point);
        self.cursors.dedup_by_key(|cursor| cursor.point);
    }

    pub fn insert(&mut self, text: &str) {
        let point = self.point;
        self.record_cursors(|edited| edited.insert(point, text));
        self.shift_marks(|mark| {
            if mark > point {
                mark + text.len()
            } else {
                mark
            }
        });
        self.mark_active = false;
        self.set_point(point + text.len());
        // Each edit at once: one diff of several would take the folds between them.
        self.follow_folds();
    }

    /// Appends output nobody typed, like a program's, unrecorded for undo; point at the
    /// end follows it, like an Emacs process marker.
    pub fn append_output(&mut self, text: &str) {
        let end = self.text.len();
        self.text.append_output(text);
        let new_end = self.text.len();
        for point in
            iter::once(&mut self.point).chain(self.window_points.iter_mut().map(|(_, point)| point))
        {
            if *point == end {
                *point = new_end;
            }
        }
    }

    /// Drops output from `len` on, unrecorded, as when a program redraws its last line.
    pub fn truncate_output(&mut self, len: usize) {
        self.text.truncate_output(len);
        self.point = self.point.min(self.text.len());
        let len = self.text.len();
        self.shift_marks(|mark| mark.min(len));
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
        self.shift_marks(shift);
        self.mark_active = false;
        self.record_cursors(|edited| edited.delete(range));
        self.set_point(point);
        self.follow_folds();
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
        if offset.is_some() {
            let before = self.cursors_before_groups.pop().unwrap_or_default();
            let now = self.place_cursors(before);
            self.cursors_undone.push(now);
        }
        self.restore_point(offset)
    }

    pub fn redo(&mut self) -> bool {
        let offset = self.text.redo();
        if offset.is_some() {
            let after = self.cursors_undone.pop().unwrap_or_default();
            let now = self.place_cursors(after);
            self.cursors_before_groups.push(now);
        }
        self.restore_point(offset)
    }

    pub fn add_undo_boundary(&mut self) {
        self.text.add_undo_boundary();
        self.cursors_at_boundary = self.cursor_places();
    }

    fn cursor_places(&self) -> Vec<CursorPlace> {
        self.cursors
            .iter()
            .map(|cursor| (cursor.point, cursor.mark, cursor.mark_active))
            .collect()
    }

    /// Moves the cursors to `places` and returns where they were. Only as many as there
    /// are now: cursors added or removed since stay as they are, not brought back.
    fn place_cursors(&mut self, places: Vec<CursorPlace>) -> Vec<CursorPlace> {
        let now = self.cursor_places();
        if places.len() == self.cursors.len() {
            for (cursor, (point, mark, mark_active)) in self.cursors.iter_mut().zip(places) {
                (cursor.point, cursor.mark, cursor.mark_active) = (point, mark, mark_active);
                cursor.goal_column = None;
            }
        }
        now
    }

    /// Makes an edit of the text, keeping the cursors as they were at the boundary when it
    /// opens an undo group; any edit ends what redo could bring back.
    fn record_cursors(&mut self, edit: impl FnOnce(&mut Text)) {
        let (depth, len) = (self.text.undo_depth(), self.text.len());
        edit(&mut self.text);
        if self.text.undo_depth() > depth {
            self.cursors_before_groups
                .push(self.cursors_at_boundary.clone());
        }
        // An empty edit records nothing, so redo still has what it had.
        if self.text.len() != len {
            self.cursors_undone.clear();
        }
    }

    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .file
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "buffer has no file"))?;
        self.text.save_as(path)
    }

    /// Reads the file again in the encoding it was read in, dropping unsaved changes and
    /// undo history; point keeps its line and column.
    pub fn revert(&mut self) -> io::Result<()> {
        let path = self
            .file
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "buffer has no file"))?;
        let format = self.text.format();
        let text = if format.binary || format.encoding == UTF_8 {
            Text::open(path)?
        } else {
            Text::open_with_encoding(path, format.encoding)?
        };
        let (line, column) = (self.point_line(), self.point_column());
        let binary = text.format().binary;
        self.replace_text(text);
        self.point = self.text.offset_at_char_column(line, column);
        if binary != format.binary {
            self.read_only = binary;
        }
        Ok(())
    }

    /// Undo replaces the whole text, so the mark may no longer fall on a character.
    fn restore_point(&mut self, offset: Option<usize>) -> bool {
        self.mark_active = false;
        let text = &self.text;
        let on_character = |mark| on_character(text, mark);
        self.mark = self.mark.map(on_character);
        for mark in &mut self.mark_ring {
            *mark = on_character(*mark);
        }
        for (_, point) in &mut self.window_points {
            *point = on_character(*point);
        }
        for cursor in &mut self.cursors {
            cursor.point = on_character(cursor.point);
            cursor.mark = cursor.mark.map(on_character);
            cursor.mark_active = false;
        }
        let restored = offset.inspect(|&offset| self.set_point(offset)).is_some();
        self.merge_cursors();
        restored
    }

    /// Moves the mark, the mark ring and the cursors with an edit, like Emacs markers.
    fn shift_marks(&mut self, shift: impl Fn(usize) -> usize) {
        self.mark = self.mark.map(&shift);
        for mark in &mut self.mark_ring {
            *mark = shift(*mark);
        }
        for cursor in &mut self.cursors {
            cursor.point = shift(cursor.point);
            cursor.mark = cursor.mark.map(&shift);
        }
        for (_, point) in &mut self.window_points {
            *point = shift(*point);
        }
    }
}

fn on_character(text: &Text, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if text.is_char_boundary(offset) {
        offset
    } else {
        text.previous_char_boundary(offset)
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
