use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::Editor;
use crate::decoration::{Span, split_by_line};
use crate::gutter::{GutterMark, LineExtra, TextControls, TextLabel};
use crate::outline::Outline;
use crate::pane::Pane;
use crate::popup::Popup;
use crate::screen::{Screen, ScreenMouse, ScreenPadding, ScreenPointer, ScreenSize};
use crate::status::StatusSegment;
use crate::table::Table;
use crate::vc::{ChangedLines, FileStatus, VersionControl};

/// Text off screen, in the middles of long lines, that decorating the visible lines at
/// once may cover; beyond it each line is decorated alone.
const MAX_UNSHOWN_DECORATED_BYTES: usize = 16 * 1024;

impl Editor {
    /// `lines` of the window buffer as a table, if a plugin draws it as one.
    pub fn table(&self, lines: Range<usize>) -> Option<Table> {
        self.registry
            .tables
            .iter()
            .find_map(|describe| describe(self, lines.clone()))
    }

    /// `lines` of the window buffer as an outline, if a plugin draws it as one.
    pub fn outline(&self, lines: Range<usize>) -> Option<Outline> {
        self.registry
            .outlines
            .iter()
            .find_map(|describe| describe(self, lines.clone()))
    }

    /// Marks beside `lines` of the window buffer, sorted by line; none when no plugin has
    /// marks for the buffer.
    pub fn gutter_marks(&self, lines: Range<usize>) -> Option<Vec<GutterMark>> {
        let mut marks: Option<Vec<GutterMark>> = None;
        for provide in &self.registry.gutters {
            if let Some(more) = provide(self, lines.clone()) {
                marks.get_or_insert_default().extend(more);
            }
        }
        if let Some(marks) = &mut marks {
            marks.sort_by_key(|mark| mark.line);
        }
        marks
    }

    pub fn line_extras(&self, lines: Range<usize>) -> Vec<LineExtra> {
        self.registry
            .line_extras
            .iter()
            .flat_map(|provide| provide(self, lines.clone()))
            .collect()
    }

    pub fn text_labels(&self, lines: Range<usize>) -> Vec<TextLabel> {
        self.registry
            .text_labels
            .iter()
            .flat_map(|provide| provide(self, lines.clone()))
            .collect()
    }

    /// The pane shown under the window buffer: the one last selected while it has
    /// something to show, else the first that has.
    pub fn shown_pane(&self) -> Option<&'static str> {
        let mut shown = self
            .registry
            .panes
            .iter()
            .filter(|source| (source.shown)(self))
            .map(|source| source.name);
        let first = shown.next();
        match self.selected_pane {
            Some(selected) if first == Some(selected) || shown.any(|name| name == selected) => {
                Some(selected)
            }
            _ => first,
        }
    }

    /// The shown pane, described.
    pub fn pane(&self) -> Option<Pane> {
        let name = self.shown_pane()?;
        let source = self
            .registry
            .panes
            .iter()
            .find(|source| source.name == name)?;
        (source.describe)(self)
    }

    /// The panes with something to show, in order, and their counts, for the switch
    /// between them; none while only one has.
    pub fn pane_switch(&self) -> Vec<(&'static str, Option<usize>)> {
        let shown: Vec<_> = self
            .registry
            .panes
            .iter()
            .filter(|source| (source.shown)(self))
            .collect();
        if shown.len() < 2 {
            return Vec::new();
        }
        shown
            .into_iter()
            .map(|source| {
                let count = (source.describe)(self).and_then(|pane| pane.count);
                (source.name, count)
            })
            .collect()
    }

    /// Shows the pane named `name` whenever it has something to show.
    pub fn select_pane(&mut self, name: &'static str) {
        self.selected_pane = Some(name);
    }

    /// Gives the keys to the shown pane, or back to the window buffer.
    pub fn focus_pane(&mut self, focused: bool) {
        self.pane_focused = focused;
    }

    /// Whether the shown pane has the keys, unless a prompt has them.
    pub fn pane_focused(&self) -> bool {
        self.pane_focused && self.minibuffer.is_none() && self.shown_pane().is_some()
    }

    /// Whether the keys go to `buffer()`, the prompt's or the window's, not to a pane;
    /// a keymap of the window buffer's mode holds only while they do.
    pub fn buffer_has_keys(&self) -> bool {
        !self.pane_focused() && self.displayed.is_none()
    }

    /// Whether the pane named `name` is shown and has the keys.
    pub fn pane_has_keys(&self, name: &str) -> bool {
        self.pane_focused() && self.shown_pane() == Some(name)
    }

    /// The buffer named `name` as a screen of `size`, if a plugin draws it as one.
    pub fn screen(&self, name: &str, size: ScreenSize) -> Option<Screen> {
        let buffer = self.buffers.iter().find(|buffer| buffer.name() == name)?;
        self.registry
            .screens
            .iter()
            .find_map(|source| source.screen(self, buffer, size))
    }

    /// Whether the buffer named `name` is drawn as a screen.
    pub fn is_screen(&self, name: &str) -> bool {
        self.buffers
            .iter()
            .find(|buffer| buffer.name() == name)
            .is_some_and(|buffer| {
                self.registry
                    .screens
                    .iter()
                    .any(|source| source.draws(buffer))
            })
    }

    /// Room around the cells of the buffer named `name`, drawn as a screen.
    pub fn screen_padding(&self, name: &str) -> ScreenPadding {
        let Some(buffer) = self.buffers.iter().find(|buffer| buffer.name() == name) else {
            return ScreenPadding::default();
        };
        self.registry
            .screens
            .iter()
            .find(|source| source.draws(buffer))
            .map(|source| source.padding(self))
            .unwrap_or_default()
    }

    /// Passes the mouse over a screen's cell to the plugins that draw screens; true when
    /// the screen changed with it. A move changes nothing else.
    pub fn screen_mouse(&mut self, name: &str, pointer: ScreenPointer) -> bool {
        let moved = matches!(pointer.mouse, ScreenMouse::Move | ScreenMouse::Leave);
        if matches!(pointer.mouse, ScreenMouse::Press { .. }) {
            self.clear_echo();
        }
        let mut changed = false;
        for source in self.registry.screens.clone() {
            changed |= source.mouse(self, name, pointer);
        }
        if !moved {
            self.after_command();
        }
        changed
    }

    /// What a click beside a line without a gutter mark runs, if anything.
    pub fn gutter_click(&self) -> Option<&'static str> {
        self.registry.gutter_click
    }

    /// Controls of the window buffer, if a plugin has any for it.
    pub fn text_controls(&self) -> Option<TextControls> {
        self.registry
            .text_controls
            .iter()
            .find_map(|describe| describe(self))
    }

    /// The version control keeping `path`, if any.
    pub fn version_control(&self, path: &Path) -> Option<Rc<dyn VersionControl>> {
        self.registry
            .version_controls
            .iter()
            .find(|control| control.controls(self, path))
            .cloned()
    }

    /// How `path` differs from the last commit, if version control knows.
    pub fn file_status(&self, path: &Path) -> Option<FileStatus> {
        self.version_control(path)?.status(self, path)
    }

    /// How the window buffer's `lines` differ from the last commit, if version control
    /// knows.
    pub fn line_changes(&self, lines: Range<usize>) -> Vec<ChangedLines> {
        let buffer = self.window_buffer();
        buffer
            .path()
            .and_then(|path| self.version_control(path))
            .map(|control| control.line_changes(buffer, lines))
            .unwrap_or_default()
    }

    /// Tells version control of files moved, from and to, as its move command would.
    pub fn record_moves(&mut self, moves: &[(PathBuf, PathBuf)]) {
        for control in self.registry.version_controls.clone() {
            let kept: Vec<_> = moves
                .iter()
                .filter(|(from, _)| {
                    self.version_control(from)
                        .is_some_and(|keeper| Rc::ptr_eq(&keeper, &control))
                })
                .cloned()
                .collect();
            if !kept.is_empty() {
                control.record_moves(self, &kept);
            }
        }
    }

    /// Faces for `range` of the window buffer, one sorted list per decorator.
    pub fn decorations(&self, range: Range<usize>) -> Vec<Vec<Span>> {
        self.registry
            .decorators
            .iter()
            .map(|decorate| decorate(self.window_buffer(), range.clone()))
            .collect()
    }

    /// Faces for each of the sorted `lines` (byte ranges) of the window buffer, one list
    /// per decorator, as [`Editor::decorations`] gives them for each line alone; asks each
    /// decorator once for each run of them that does not cover far more text than they
    /// show, as lines cut short of their very long middles, or lines folded, do.
    pub fn decorations_by_line(&self, lines: &[Range<usize>]) -> Vec<Vec<Vec<Span>>> {
        let mut by_line = Vec::with_capacity(lines.len());
        let mut start = 0;
        while start < lines.len() {
            let (mut end, mut shown) = (start + 1, lines[start].len());
            while let Some(next) = lines.get(end) {
                let covered = next.end - lines[start].start;
                if covered > 2 * (shown + next.len()) + MAX_UNSHOWN_DECORATED_BYTES {
                    break;
                }
                (end, shown) = (end + 1, shown + next.len());
            }
            by_line.extend(self.decorations_at_once(&lines[start..end]));
            start = end;
        }
        by_line
    }

    /// [`Editor::decorations_by_line`] of `lines` asking each decorator once for them all.
    fn decorations_at_once(&self, lines: &[Range<usize>]) -> Vec<Vec<Vec<Span>>> {
        let all = lines[0].start..lines[lines.len() - 1].end;
        let layers = self.decorations(all);
        let mut by_line: Vec<Vec<Vec<Span>>> = vec![Vec::with_capacity(layers.len()); lines.len()];
        for layer in &layers {
            for (line, spans) in by_line.iter_mut().zip(split_by_line(layer, lines)) {
                line.push(spans);
            }
        }
        by_line
    }

    pub fn status_segments(&self) -> Vec<StatusSegment> {
        self.registry
            .status_providers
            .iter()
            .filter_map(|describe| describe(self))
            .collect()
    }

    pub fn popups(&self) -> Vec<Popup> {
        self.popup_slots().flatten().collect()
    }

    /// Each plugin's popup, if it has one open, in the same place every time, so a popup
    /// is known as the same one from one frame to the next.
    pub fn popup_slots(&self) -> impl Iterator<Item = Option<Popup>> + '_ {
        self.registry
            .popup_providers
            .iter()
            .map(|describe| describe(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{Plugin, Registry};

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn decorations_by_line_ask_once_for_near_lines_and_alone_for_far_ones() {
        use std::cell::RefCell;

        thread_local! {
            static ASKED: RefCell<Vec<Range<usize>>> = const { RefCell::new(Vec::new()) };
        }
        struct Whole;
        impl Plugin for Whole {
            fn register(&self, registry: &mut Registry) {
                registry.decorators.push(Rc::new(|_, range: Range<usize>| {
                    ASKED.with_borrow_mut(|asked| asked.push(range.clone()));
                    vec![Span {
                        range,
                        face: crate::Face::Keyword,
                    }]
                }));
            }
        }
        let editor = Editor::new(&[&Whole]);
        let decorate = |lines: &[Range<usize>]| {
            let by_line: Vec<Vec<Range<usize>>> = editor
                .decorations_by_line(lines)
                .into_iter()
                .map(|layers| layers[0].iter().map(|span| span.range.clone()).collect())
                .collect();
            (by_line, ASKED.take())
        };
        assert_eq!(
            decorate(&[0..10, 11..21]),
            (vec![vec![0..10], vec![11..21]], vec![0..21])
        );
        let far = 100_000..100_010;
        assert_eq!(
            decorate(&[0..10, far.clone()]),
            (vec![vec![0..10], vec![far.clone()]], vec![0..10, far])
        );
        // Lines on each side of lines folded away, each side asked for once.
        let after_fold = [100_000..100_010, 100_011..100_021];
        assert_eq!(
            decorate(&[0..10, 11..21, after_fold[0].clone(), after_fold[1].clone()]).1,
            vec![0..21, 100_000..100_021]
        );
        assert_eq!(decorate(&[]), (Vec::new(), Vec::new()));
    }

    #[test]
    fn the_selected_pane_is_shown_while_it_has_something_to_show() {
        #[derive(Default)]
        struct Shown(bool, bool);
        struct Panes;
        impl Plugin for Panes {
            fn register(&self, registry: &mut Registry) {
                fn a(editor: &Editor) -> bool {
                    editor
                        .find_plugin_state::<Shown>()
                        .is_some_and(|shown| shown.0)
                }
                fn b(editor: &Editor) -> bool {
                    editor
                        .find_plugin_state::<Shown>()
                        .is_some_and(|shown| shown.1)
                }
                for (name, shown) in [("A", a as fn(&Editor) -> bool), ("B", b)] {
                    registry.panes.push(crate::pane::PaneSource {
                        name,
                        shown,
                        describe: Rc::new(|_| None),
                    });
                }
            }
        }
        let mut editor = Editor::new(&[&Panes]);
        let show = |editor: &mut Editor, a, b| *editor.plugin_state::<Shown>() = Shown(a, b);
        assert_eq!(editor.shown_pane(), None);
        show(&mut editor, true, true);
        assert_eq!(editor.shown_pane(), Some("A"));
        assert_eq!(editor.pane_switch(), [("A", None), ("B", None)]);
        editor.select_pane("B");
        assert_eq!(editor.shown_pane(), Some("B"));
        show(&mut editor, true, false);
        assert_eq!(editor.shown_pane(), Some("A"));
        assert!(editor.pane_switch().is_empty());
        editor.focus_pane(true);
        assert!(editor.pane_has_keys("A") && !editor.buffer_has_keys());
        show(&mut editor, false, false);
        assert!(!editor.pane_focused() && editor.buffer_has_keys());
    }
}
