use std::mem;
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    App, Bounds, Context, DispatchPhase, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, ScrollWheelEvent, Window, canvas, prelude::*, px,
};
use remacs_core::{Action, Axis, MouseSelection, ScreenMouse};
use remacs_theme::font;

use super::screen::screen_button;
use super::{EditorView, Metrics, ROW_MARGIN, TEXT_TOP};
use crate::outline_view::{COMMIT_HEIGHT, fitting_commits};
use crate::smooth_scroll::wheel_delta;
use crate::table_view::ROW_PITCH;

impl EditorView {
    pub(super) fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.modifiers.platform {
            if self.text_zoom.scroll(event.delta, font::code().size, self.zoom) {
                cx.notify();
            }
            return;
        }
        // The wheel scrolls the window under the mouse, selected or not.
        self.in_window_at(event.position, |view| {
            let rows = view.window_state.showing_rows;
            let row_height = px(if rows { ROW_PITCH } else { view.text_font().line_height });
            let (delta, at_once) = wheel_delta(event, row_height, view.scroll.lines_per_notch);
            // A swipe is never quite straight: it scrolls along its main direction only.
            if delta.x.abs() > delta.y.abs() {
                view.scroll_sideways(-delta.x, at_once, cx);
                return;
            }
            if view.drawing_selected() {
                // Point may move with the text, and the marked text with it.
                view.cancel_composition(cx);
            }
            view.scroll_vertically(-delta.y / row_height, at_once);
            if view.drawing_selected() {
                view.drag_selection();
            }
            cx.notify();
        });
    }

    /// Scrolls by `rows` lines or rows, fractions included, leaving point where it is
    /// unless it would leave the window; `at_once` skips the glide.
    fn scroll_vertically(&mut self, rows: f32, at_once: bool) {
        let shown = self.editor.window_buffer().shown_lines();
        let first_row = shown.row_of(self.editor.viewport.first_line);
        let last_start = shown.rows() - 1;
        let position =
            (self.window_state.vertical.target(first_row) + rows).clamp(0.0, last_start as f32);
        self.editor.scroll(position as isize - first_row as isize);
        let first_row = shown.row_of(self.editor.viewport.first_line);
        self.window_state.vertical.rest_at(first_row, position - first_row as f32, at_once);
    }

    /// Scrolls the text sideways by `delta`, no further right than the drawn lines go,
    /// leaving point where it is.
    fn scroll_sideways(&mut self, delta: Pixels, at_once: bool, cx: &mut Context<Self>) {
        let Some(metrics) = self.text_metrics() else {
            return;
        };
        let columns = delta / metrics.char_width;
        if columns > 0.0 && !self.window_state.more_on_right {
            return;
        }
        let viewport = &mut self.editor.viewport;
        let position =
            (self.window_state.sideways.target(viewport.first_column) + columns).max(0.0);
        viewport.first_column = position as usize;
        self.window_state.sideways.rest_at(viewport.first_column, position.fract(), at_once);
        self.window_state.scrolled_sideways = true;
        cx.notify();
    }

    /// Scrolls an outline's history by what the wheel moved, part of a commit included;
    /// the plugin is told as whole commits are scrolled past.
    pub fn scroll_history(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        self.in_window_at(event.position, |view| view.scroll_history_here(event, cx));
    }

    fn scroll_history_here(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let row_height = px(COMMIT_HEIGHT);
        let (delta, at_once) = wheel_delta(event, row_height, self.scroll.lines_per_notch);
        let state = &mut self.window_state;
        // Nothing is above the newest commit or below the oldest.
        let lowest = if state.history_scrolled.above { f32::NEG_INFINITY } else { 0.0 };
        let highest = if state.history_scrolled.below { f32::INFINITY } else { 0.0 };
        let position = (state.history.target() - delta.y / row_height).clamp(lowest, highest);
        state.history_target = position;
        if at_once {
            state.history.place(position);
        }
        cx.notify();
    }

    /// Glides the history toward where the wheel scrolled it and moves its first commit
    /// past those scrolled over; returns commits drawn past the first, a fraction of one.
    pub(super) fn advance_history(&mut self, window: &Window, cx: &mut Context<Self>) -> f32 {
        let duration = self.scroll.animation_duration();
        let max_distance = fitting_commits(self.window_state.frame.size.height, false) as f32;
        let state = &mut self.window_state;
        let target = state.history_target;
        let shown = state.history.advance(target, Instant::now(), duration, max_distance);
        if state.history.is_animating() {
            window.request_animation_frame();
        }
        let whole = shown.floor();
        if whole != 0.0
            && let Some(scroll) = state.history_scroll.clone()
        {
            let rows = whole as isize;
            let action: Action = Rc::new(move |editor| scroll(editor, rows));
            self.run_action(&action, cx);
            self.window_state.history.shift(-whole);
            self.window_state.history_target -= whole;
        }
        self.window_state.history.shown().max(0.0)
    }

    pub(super) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        if mem::take(&mut self.message_open) | mem::take(&mut self.hints_open) {
            cx.notify();
        }
        // The window under the mouse was selected as the click went down to it.
        self.follow_selection();
        // An open prompt is modal: clicks choose candidates or do nothing. Rows handle
        // their own clicks.
        let Some(metrics) = self.text_metrics().filter(|_| self.editor.minibuffer().is_none())
        else {
            return;
        };
        let Some(line) = self.line_at(event.position.y) else {
            return;
        };
        let offset = self.offset_at(line, event.position.x, metrics);
        let buffer = self.editor.window_buffer_mut();
        let (clicks, extend) = (event.click_count, event.modifiers.shift);
        self.mouse_selection = Some(MouseSelection::press(buffer, offset, clicks, extend));
        self.drag_position = event.position;
        self.window_state.scrolled_sideways = false;
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if let Some(divider) = self.divider_drag {
            if event.pressed_button != Some(MouseButton::Left) {
                self.divider_drag = None;
                return;
            }
            let Bounds { origin, size } = divider.bounds;
            let (offset, length) = match divider.axis {
                Axis::Rows => (event.position.y - origin.y, size.height),
                Axis::Columns => (event.position.x - origin.x, size.width),
            };
            self.editor.set_split_ratio(divider.split, offset / length, divider.min / length);
            cx.notify();
            return;
        }
        if let Some((area, button)) = self.screen_press.clone() {
            let mouse = if event.pressed_button == Some(button) {
                ScreenMouse::Drag
            } else {
                // Let go outside the window.
                self.screen_press = None;
                ScreenMouse::Release { button: screen_button(button) }
            };
            let pointer = area.pointer(event.position, &event.modifiers, mouse);
            self.editor.screen_mouse(&area.buffer, pointer);
            cx.notify();
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.release_mouse();
            return;
        }
        self.drag_position = event.position;
        if self.drag_selection() != 0 && self.drag_scroll.is_none() {
            let interval = self.mouse.drag_scroll_interval();
            self.drag_scroll = Some(cx.spawn(async move |view, cx| {
                loop {
                    cx.background_executor().timer(interval).await;
                    let scrolled = view.update(cx, |view, cx| view.scroll_to_drag(cx));
                    if !matches!(scrolled, Ok(true)) {
                        break;
                    }
                }
            }));
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if event.button == MouseButton::Left && self.divider_drag.take().is_some() {
            cx.notify();
            return;
        }
        match self.screen_press.take() {
            Some((area, button)) if button == event.button => {
                let mouse = ScreenMouse::Release { button: screen_button(button) };
                let pointer = area.pointer(event.position, &event.modifiers, mouse);
                self.editor.screen_mouse(&area.buffer, pointer);
            }
            pressed => {
                self.screen_press = pressed;
                if event.button != MouseButton::Left {
                    return;
                }
                self.release_mouse();
            }
        }
        cx.notify();
    }

    /// The metrics the text was last drawn with, none while the window shows rows.
    fn text_metrics(&self) -> Option<Metrics> {
        let (_, metrics) = self.metrics.as_ref().filter(|_| !self.window_state.showing_rows)?;
        Some(*metrics)
    }

    fn release_mouse(&mut self) {
        self.mouse_selection = None;
        self.drag_scroll = None;
    }

    /// Scrolls a line toward the mouse held past the text's top or bottom, extending the
    /// selection; false once the mouse is back over the text.
    fn scroll_to_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let direction = self.drag_selection();
        if direction == 0 {
            self.drag_scroll = None;
            return false;
        }
        let first_line = self.editor.viewport.first_line;
        self.editor.scroll(direction);
        if self.editor.viewport.first_line != first_line {
            self.drag_selection();
            cx.notify();
        }
        true
    }

    /// Extends the selection to the mouse, or to the nearest line point can be on without
    /// scrolling; returns which way the text must scroll to reach the mouse.
    fn drag_selection(&mut self) -> isize {
        let (Some(selection), Some((_, metrics))) = (&self.mouse_selection, &self.metrics) else {
            return 0;
        };
        let (selection, metrics) = (selection.clone(), *metrics);
        let viewport = &self.editor.viewport;
        let line_count = self.editor.window_buffer().text().line_count();
        let shown = self.editor.window_buffer().shown_lines();
        // Above the text counts as the first line, which is above any line point is on
        // with the text scrolled.
        let under = self.line_at(self.drag_position.y).unwrap_or(0);
        let line = viewport.clamp(under, &shown);
        let offset = self.offset_at(line, self.drag_position.x, metrics);
        selection.drag(self.editor.window_buffer_mut(), offset);
        self.window_state.scrolled_sideways = false;
        // Past the last line there is nothing more to scroll to.
        let last_line = line_count.saturating_sub(1);
        under.min(last_line).cmp(&line.min(last_line)) as isize
    }

    /// The line drawn at `y` in the frame, none above the text, the line count below it.
    fn line_at(&self, y: Pixels) -> Option<usize> {
        let y = y - self.window_state.frame.origin.y;
        let rows = (y - px(TEXT_TOP)) / px(self.text_font().line_height);
        let row = (rows >= 0.0).then(|| (self.window_state.vertical.shown() + rows) as usize)?;
        let buffer = self.editor.window_buffer();
        let shown = buffer.shown_lines();
        Some(if row < shown.rows() { shown.line_at(row) } else { buffer.text().line_count() })
    }

    /// The offset in the window buffer nearest to `x` on `line`; past the last line, the
    /// end of the text.
    fn offset_at(&self, line: usize, x: Pixels, metrics: Metrics) -> usize {
        let x = x - self.window_state.frame.origin.x;
        let text_left = self.window_state.text_left + px(ROW_MARGIN) + self.gutter_width(metrics);
        let columns = (x - text_left) / metrics.char_width + self.window_state.sideways.shown();
        let display_column = columns.round().max(0.0) as usize;
        let column = self.display_line(line).0.char_column_at(display_column);
        let text = self.editor.window_buffer().text();
        text.offset_at_char_column(line, column)
    }

    /// Follows the mouse anywhere, even outside the window, while a selection is dragged.
    pub(super) fn render_drag_listeners(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.mouse_selection.is_none()
            && self.screen_press.is_none()
            && self.divider_drag.is_none()
        {
            return None;
        }
        let view = cx.entity().downgrade();
        let paint = move |_, _, window: &mut Window, _: &mut App| {
            let moved = view.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    let _ = moved.update(cx, |view, cx| view.on_mouse_move(event, cx));
                }
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    let _ = view.update(cx, |view, cx| view.on_mouse_up(event, cx));
                }
            });
        };
        Some(canvas(|_, _, _| {}, paint).absolute().size_full())
    }
}
