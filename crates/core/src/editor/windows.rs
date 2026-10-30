use std::io;
use std::mem;
use std::path::Path;

use super::Editor;
use crate::buffer::BufferId;
use crate::command::CommandResult;
use crate::window::{Axis, Layout, WindowId};

/// The window [`Editor::display_window`] made the window buffer, and where its buffer
/// was in the buffer list, to put back.
pub(super) struct Displayed {
    window: WindowId,
    index: usize,
}

impl Editor {
    pub fn layout(&self) -> &Layout {
        &self.windows.layout
    }

    pub fn selected_window(&self) -> WindowId {
        self.windows.selected()
    }

    /// The window that the window buffer and the viewport are now: the selected one,
    /// unless another is displayed.
    pub fn displayed_window(&self) -> WindowId {
        self.displayed
            .as_ref()
            .map_or(self.windows.selected(), |displayed| displayed.window)
    }

    pub fn window_count(&self) -> usize {
        self.windows.recent.len()
    }

    /// If another window shows the selected window's buffer too, shows in the selected
    /// one the most recent buffer no window shows, else the next most recent one, and
    /// returns true; the buffer stays as it is in the others.
    pub fn leave_shared_buffer(&mut self) -> bool {
        let shown = self.buffers[0].id();
        let in_windows: Vec<BufferId> = self.windows.other_buffers().collect();
        if !in_windows.contains(&shown) {
            return false;
        }
        let replacement = self
            .buffers
            .iter()
            .position(|buffer| buffer.id() != shown && !in_windows.contains(&buffer.id()))
            .or((self.buffers.len() > 1).then_some(1));
        replacement
            .inspect(|&index| self.select_buffer(index))
            .is_some()
    }

    /// The window selected before the selected one, where Emacs `*-other-window` commands
    /// show buffers.
    pub fn previous_window(&self) -> Option<WindowId> {
        self.windows.recent.get(1).copied()
    }

    /// Gives the selected window `cells` more lines or columns along `axis`, fewer if
    /// negative, from the windows beside it, leaving each at least `min`; false if none
    /// can change.
    pub fn resize_window(&mut self, axis: Axis, cells: isize, min: usize) -> bool {
        self.end_display();
        let selected = self.windows.selected();
        let size = match axis {
            Axis::Rows => self.viewport.height,
            Axis::Columns => self.viewport.width,
        };
        // 0 until the window is first laid out.
        let share = self.windows.layout.share(selected, axis);
        let Some(share) = share.filter(|_| size > 0) else {
            return false;
        };
        let whole = size as f32 / share;
        self.windows
            .layout
            .resize(selected, axis, cells as f32 / whole, min as f32 / whole)
    }

    /// Sets the share of the first side of the `split`th split, counted depth first, as
    /// dragging its divider does, leaving each window at least `min` of the split.
    pub fn set_split_ratio(&mut self, split: usize, ratio: f32, min: f32) {
        self.windows.layout.set_ratio(split, ratio, min);
    }

    pub fn balance_windows(&mut self) {
        self.windows.layout.balance();
    }

    /// Halves the selected window, like Emacs `split-window`; the new window, below or on
    /// the right, shows the same buffer at the same point and scroll.
    pub fn split_window(&mut self, axis: Axis) {
        let buffer = &mut self.buffers[0];
        let new = self.windows.split(axis, buffer.id(), self.viewport);
        buffer.park_point(new);
    }

    /// Deletes the selected window and selects the one selected before it.
    pub fn delete_window(&mut self) -> CommandResult {
        let deleted = self.windows.selected();
        let previous = self
            .previous_window()
            .ok_or("Can't delete the only window")?;
        self.select_window(previous);
        self.remove_window(deleted);
        Ok(())
    }

    pub fn delete_other_windows(&mut self) {
        let others: Vec<WindowId> = self.windows.recent[1..].into();
        for window in others {
            self.remove_window(window);
        }
    }

    /// Gives `window` the keys; its buffer becomes the most recent.
    pub fn select_window(&mut self, window: WindowId) {
        self.end_display();
        let selected = self.windows.selected();
        let Some(other) = self.windows.other_mut(window) else {
            return;
        };
        // The entry `window` leaves is the one the selected window takes.
        other.id = selected;
        let buffer = mem::replace(&mut other.buffer, self.buffers[0].id());
        let viewport = mem::replace(&mut other.viewport, self.viewport);
        self.buffers[0].park_point(selected);
        let index = self.buffer_position(buffer);
        self.select_buffer(index);
        self.viewport = viewport;
        let buffer = &mut self.buffers[0];
        buffer.unpark_point(window);
        buffer.set_point(buffer.point());
        self.windows.recent.retain(|&id| id != window);
        self.windows.recent.insert(0, window);
    }

    /// Makes `window` the window buffer, with its point and viewport, until
    /// [`Editor::end_display`], to draw it or keep what is drawn of it current, as Emacs
    /// redisplay does; for the selected window it does nothing. The buffer order stays,
    /// no hooks run, and the buffer has no keys meanwhile.
    pub fn display_window(&mut self, window: WindowId) {
        self.end_display();
        let Some(other) = self.windows.other_mut(window) else {
            return;
        };
        mem::swap(&mut self.viewport, &mut other.viewport);
        let buffer = other.buffer;
        let index = self.buffer_position(buffer);
        self.buffers.swap(0, index);
        let buffer = &mut self.buffers[0];
        buffer.park_point(WindowId::PARKED);
        buffer.unpark_point(window);
        self.displayed = Some(Displayed { window, index });
    }

    /// Puts back what [`Editor::display_window`] swapped in.
    pub fn end_display(&mut self) {
        let Some(Displayed { window, index }) = self.displayed.take() else {
            return;
        };
        let buffer = &mut self.buffers[0];
        buffer.park_point(window);
        buffer.unpark_point(WindowId::PARKED);
        self.buffers.swap(0, index);
        if let Some(other) = self.windows.other_mut(window) {
            mem::swap(&mut self.viewport, &mut other.viewport);
        }
    }

    /// Shows the buffer visiting `path`, as from a list, like Emacs `display-buffer`: in a
    /// window that shows it already, else in the most recent window but the selected
    /// one, which stays selected; with one window, in it. `place` then moves point there.
    pub fn display_file(&mut self, path: &Path, place: impl FnOnce(&mut Editor)) -> io::Result<()> {
        let index = self.find_file(path)?;
        let buffer = self.buffers[index].id();
        let shown = self.windows.showing(buffer).next();
        let window = match shown {
            _ if index == 0 => None,
            Some(window) => Some(window),
            None => self.previous_window(),
        };
        let Some(window) = window else {
            self.select_buffer(index);
            place(self);
            return Ok(());
        };
        if shown.is_none() {
            self.show_in_window(window, buffer);
        }
        self.display_window(window);
        place(self);
        self.show_point();
        self.end_display();
        Ok(())
    }

    /// After `killed` was killed, the windows that showed it show the most recent buffer
    /// no window shows, else the most recent one.
    pub(super) fn replace_in_windows(&mut self, killed: BufferId) {
        let mut shown: Vec<BufferId> = self.windows.other_buffers().collect();
        shown.push(self.buffers[0].id());
        let replacement = self
            .buffers
            .iter()
            .find(|buffer| !shown.contains(&buffer.id()))
            .unwrap_or(&self.buffers[0])
            .id();
        let showing: Vec<WindowId> = self.windows.showing(killed).collect();
        for window in showing {
            self.show_in_window(window, replacement);
        }
    }

    /// Runs the display hooks once for each buffer only windows not selected show.
    pub(super) fn run_display_hooks(&mut self) {
        let hooks = self.registry.display_hooks.clone();
        let mut done = vec![self.buffers[0].id()];
        let others: Vec<WindowId> = self.windows.recent[1..].into();
        for window in others {
            let buffer = self.windows.other(window).map(|window| window.buffer);
            let Some(buffer) = buffer.filter(|buffer| !done.contains(buffer)) else {
                continue;
            };
            done.push(buffer);
            self.display_window(window);
            for hook in &hooks {
                hook(self);
            }
            self.end_display();
        }
    }

    /// Makes a window but the selected one show `buffer` where that buffer was last shown.
    fn show_in_window(&mut self, window: WindowId, buffer: BufferId) {
        let Some(other) = self.windows.other_mut(window) else {
            return;
        };
        let left = mem::replace(&mut other.buffer, buffer);
        let first_line = other.viewport.first_line;
        if let Some(left) = self.buffer_with_id_mut(left) {
            left.window_start = first_line;
            left.forget_window(window);
        }
        let index = self.buffer_position(buffer);
        let shown = &mut self.buffers[index];
        shown.park_point(window);
        let window_start = shown.window_start;
        if let Some(other) = self.windows.other_mut(window) {
            other.viewport.first_line = window_start;
            other.viewport.first_column = 0;
        }
    }

    fn remove_window(&mut self, window: WindowId) {
        let removed = self.windows.remove(window);
        if let Some(buffer) = removed.and_then(|removed| self.buffer_with_id_mut(removed.buffer)) {
            buffer.forget_window(window);
        }
    }

    /// Windows only show live buffers: killing one shows another in its place.
    fn buffer_position(&self, buffer: BufferId) -> usize {
        let position = self.buffers.iter().position(|shown| shown.id() == buffer);
        debug_assert!(position.is_some(), "a window shows a killed buffer");
        position.unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use remacs_text::Text;

    use super::*;
    use crate::buffer::Buffer;
    use crate::editor::SCRATCH_BUFFER;
    use crate::editor::tests::{buffer_names, editor_registering, temp_dir};

    fn editor_with(name: &str, text: &str) -> Editor {
        let mut editor = Editor::new(&[]);
        editor.add_buffer(Buffer::new(name, Text::from(text), "Fundamental"));
        editor
    }

    #[test]
    fn windows_on_one_buffer_keep_points_of_their_own_through_edits() {
        let mut editor = editor_with("a", "one\ntwo\n");
        editor.buffer_mut().set_point(4);
        editor.split_window(Axis::Rows);
        let [top, bottom] = editor.layout().windows()[..] else {
            panic!("two windows");
        };
        assert_eq!(editor.selected_window(), top);
        editor.buffer_mut().set_point(0);
        editor.buffer_mut().insert("zero\n");
        editor.display_window(bottom);
        let point = |editor: &Editor| editor.window_buffer().point();
        assert_eq!(point(&editor), 9, "moved by the insert above it");
        assert!(!editor.buffer_has_keys());
        editor.end_display();
        assert_eq!(point(&editor), 5);
        editor.select_window(bottom);
        assert_eq!(point(&editor), 9);
        editor.select_window(top);
        assert_eq!(point(&editor), 5);
    }

    #[test]
    fn displaying_a_window_swaps_its_buffer_in_and_back() {
        let mut editor = editor_with("a", "text");
        editor.viewport.first_line = 3;
        editor.split_window(Axis::Columns);
        let right = editor.layout().windows()[1];
        editor.switch_to_buffer(SCRATCH_BUFFER);
        editor.viewport.first_line = 0;
        let order = |editor: &Editor| buffer_names(editor).join(" ");
        let before = order(&editor);
        editor.display_window(right);
        assert_eq!(editor.window_buffer().name(), "a");
        assert_eq!(editor.viewport.first_line, 3);
        assert_eq!(editor.displayed_window(), right);
        editor.end_display();
        assert_eq!(order(&editor), before);
        assert_eq!(editor.viewport.first_line, 0);
    }

    #[test]
    fn deleting_windows_selects_the_one_selected_before() {
        let mut editor = editor_with("a", "");
        assert!(editor.delete_window().is_err());
        editor.split_window(Axis::Rows);
        editor.split_window(Axis::Columns);
        let [first, right, below] = editor.layout().windows()[..] else {
            panic!("three windows");
        };
        editor.select_window(below);
        editor.select_window(right);
        editor.delete_window().unwrap();
        assert_eq!(editor.selected_window(), below);
        assert_eq!(editor.layout().windows(), [first, below]);
        editor.delete_other_windows();
        assert_eq!(editor.layout(), &Layout::Window(below));
        assert_eq!(editor.window_count(), 1);
    }

    #[test]
    fn resizing_counts_the_selected_window_s_lines_and_balancing_undoes_it() {
        let mut editor = editor_with("a", "");
        editor.split_window(Axis::Rows);
        assert!(!editor.resize_window(Axis::Rows, 4, 4), "not laid out yet");
        editor.viewport.height = 20;
        assert!(editor.resize_window(Axis::Rows, 4, 4));
        let ratio = |editor: &Editor| match editor.layout() {
            Layout::Split { ratio, .. } => *ratio,
            Layout::Window(_) => panic!("a split"),
        };
        assert!((ratio(&editor) - 0.6).abs() < 1e-6, "24 of 40 lines");
        editor.viewport.height = 24;
        assert!(editor.resize_window(Axis::Rows, 100, 4));
        let kept = (ratio(&editor) - 0.9).abs() < 1e-6;
        assert!(kept, "the other keeps 4 lines");
        assert!(!editor.resize_window(Axis::Columns, 1, 4));
        editor.balance_windows();
        assert_eq!(ratio(&editor), 0.5);
    }

    #[test]
    fn a_killed_buffer_gives_its_windows_one_no_window_shows() {
        let mut editor = editor_with("a", "");
        editor.add_buffer(Buffer::new("b", Text::default(), "Fundamental"));
        editor.split_window(Axis::Rows);
        let below = editor.layout().windows()[1];
        editor.switch_to_buffer("a");
        editor.kill_buffer("b");
        editor.display_window(below);
        assert_eq!(editor.window_buffer().name(), SCRATCH_BUFFER);
        editor.end_display();
        assert_eq!(editor.window_buffer().name(), "a");
    }

    #[test]
    fn a_file_shown_from_a_list_goes_to_another_window() {
        let directory = temp_dir("windows");
        let file = directory.join("shown.txt");
        std::fs::write(&file, "one\ntwo\n").unwrap();
        let mut editor = editor_with("list", "");
        let at = |line, column| move |editor: &mut Editor| editor.set_point_at(line, column);
        editor.display_file(&file, at(1, 0)).unwrap();
        let name = |editor: &Editor| editor.window_buffer().name().to_owned();
        assert_eq!(name(&editor), "shown.txt", "one window: in it");
        editor.switch_to_buffer("list");
        editor.split_window(Axis::Columns);
        let right = editor.layout().windows()[1];
        editor.display_file(&file, at(0, 2)).unwrap();
        assert_eq!(name(&editor), "list", "stays selected");
        editor.select_window(right);
        assert_eq!(name(&editor), "shown.txt");
        assert_eq!(editor.window_buffer().point(), 2);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn display_hooks_run_once_for_each_buffer_only_other_windows_show() {
        #[derive(Default)]
        struct Seen(Vec<String>);
        let mut editor = editor_registering(|registry| {
            registry.display_hooks.push(std::rc::Rc::new(|editor| {
                let name = editor.window_buffer().name().to_owned();
                editor.plugin_state::<Seen>().0.push(name);
            }));
        });
        editor.add_buffer(Buffer::new("a", Text::default(), "Fundamental"));
        editor.add_buffer(Buffer::new("b", Text::default(), "Fundamental"));
        editor.split_window(Axis::Rows);
        editor.split_window(Axis::Rows);
        editor.switch_to_buffer("a");
        editor.split_window(Axis::Columns);
        editor.refresh();
        assert_eq!(editor.plugin_state::<Seen>().0, ["b"]);
        assert_eq!(editor.window_buffer().name(), "a");
    }
}
