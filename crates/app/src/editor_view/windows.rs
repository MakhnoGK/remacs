use std::collections::HashMap;
use std::iter;
use std::mem;

use gpui::{
    AnyElement, Bounds, Context, Div, MouseButton, MouseDownEvent, Pixels, Point, Window, div,
    point, prelude::*, px, rgb, rgba,
};
use remacs_core::{Axis, Layout, WindowId};
use remacs_theme::{color, font, glass};

use super::screen::full_screen_bounds;
use super::{Divider, EditorView, ROW_MARGIN};
use crate::bottom_row::Hints;
use crate::changes_view::render_changes;
use crate::outline_view::{
    fits_beside, max_outline_lines, render_text_side, render_toolbar, text_side_end,
};
use crate::table_view::{
    LIST_BOTTOM, ROW_PITCH, ROWS_TOP, TableCursor, fits_preview, render_table,
};
use crate::widgets::Scrolled;

/// What a window is drawn as, and what of it goes elsewhere: its header's toolbar, its
/// keys for the bottom row, its popups.
pub(super) struct WindowParts {
    pub(super) body: AnyElement,
    pub(super) toolbar: Option<AnyElement>,
    /// Chips name what it shows, in place of its title.
    pub(super) tabbed: bool,
    pub(super) hints: Hints,
    pub(super) popups: Vec<AnyElement>,
    /// Drawn as text, whose popups they are.
    pub(super) text_shown: bool,
}

impl EditorView {
    /// Makes the editor display `window`, and `window_state` its, until
    /// [`EditorView::leave_window`]; the selected window needs neither.
    pub(super) fn enter_window(&mut self, window: WindowId) {
        self.editor.display_window(window);
        self.switch_state(window);
    }

    pub(super) fn leave_window(&mut self) {
        self.editor.end_display();
        self.follow_selection();
    }

    /// Runs `act` with the window drawn at `position` entered, if there is one.
    pub(super) fn in_window_at<R>(
        &mut self,
        position: Point<Pixels>,
        act: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let under = iter::once((&self.state_window, &self.window_state))
            .chain(&self.other_states)
            .find(|(_, state)| state.area.contains(&position))
            .map(|(window, _)| *window);
        if let Some(window) = under {
            self.enter_window(window);
        }
        let result = act(self);
        self.leave_window();
        result
    }

    /// Makes `window_state` the selected window's again, after a command or a click
    /// selected another.
    pub(super) fn follow_selection(&mut self) {
        self.switch_state(self.editor.selected_window());
    }

    fn switch_state(&mut self, window: WindowId) {
        if window == self.state_window {
            return;
        }
        let state = self.other_states.remove(&window).unwrap_or_default();
        let left = mem::replace(&mut self.window_state, state);
        let left_window = mem::replace(&mut self.state_window, window);
        self.other_states.insert(left_window, left);
    }

    /// Whether the window being drawn or handled is the selected one.
    pub(super) fn drawing_selected(&self) -> bool {
        self.editor.displayed_window() == self.editor.selected_window()
    }

    /// A click on a window not selected selects it, before what was clicked takes it.
    pub(super) fn select_clicked(&mut self, window: WindowId, cx: &mut Context<Self>) {
        if window != self.editor.selected_window() {
            self.editor.select_window(window);
            self.editor.focus_pane(false);
            self.follow_selection();
            cx.notify();
        }
    }

    /// For each screen windows show, the window that sizes it: the selected one if it
    /// shows it, else the first in order on screen (ADR 0073).
    pub(super) fn screen_owners(&mut self, windows: &[WindowId]) -> HashMap<String, WindowId> {
        let selected = self.editor.selected_window();
        let mut owners = HashMap::new();
        for &window in windows {
            self.editor.display_window(window);
            let name = self.editor.window_buffer().name();
            if self.editor.is_screen(name) {
                let name = name.to_owned();
                if window == selected {
                    owners.insert(name, window);
                } else {
                    owners.entry(name).or_insert(window);
                }
            }
            self.editor.end_display();
        }
        owners
    }

    /// The window the editor displays, laid out in `window_state.frame`.
    pub(super) fn render_window(
        &mut self,
        screen_owners: &HashMap<String, WindowId>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> WindowParts {
        let window_name = self.editor.window_buffer().name().to_owned();
        self.window_state.showing_screen = self.editor.is_screen(&window_name);
        let frame_size = self.window_state.frame.size;
        let reserved = self.window_state.reserved;
        // Tables and outlines are laid out in the window above the pane.
        let rows_area_height = frame_size.height - (reserved - px(LIST_BOTTOM)).max(px(0.0));
        let first_line = self.editor.viewport.first_line;
        let table_rows = ((rows_area_height - px(ROWS_TOP) - px(LIST_BOTTOM)) / px(ROW_PITCH))
            .floor()
            .max(1.0) as usize;
        // One row more than fits is partly scrolled in; another tells whether there are
        // rows below.
        let table = (!self.window_state.showing_screen)
            .then(|| self.editor.table(first_line..first_line + table_rows + 2))
            .flatten();
        let outline = match table {
            Some(_) => None,
            None if self.window_state.showing_screen => None,
            None => {
                let lines = max_outline_lines(rows_area_height);
                self.render_outline(lines, rows_area_height, window, cx)
            }
        };
        self.window_state.showing_rows = table.is_some() || outline.is_some();
        let mut toolbar = None;
        let mut tabbed = false;
        let mut text_shown = false;
        self.window_state.text_left = px(0.0);
        self.window_state.number_width = None;
        let (body, hints, popups) = match (table, outline) {
            _ if self.window_state.showing_screen => {
                let controls = self.editor.text_controls().unwrap_or_default();
                tabbed = controls.tabs.is_some();
                toolbar = self.render_header_controls(controls.tabs, controls.buttons, cx);
                let bounds = full_screen_bounds(frame_size, reserved);
                let owner = screen_owners.get(&window_name).copied();
                let body = if owner.is_some_and(|owner| owner != self.editor.displayed_window()) {
                    owner.map(|owner| render_shown_elsewhere(owner, cx))
                } else {
                    let offset = self.window_state.frame.origin;
                    let under = rgba(glass::window_tint()).into();
                    self.render_screen_in(&window_name, bounds, offset, under, window, cx)
                };
                let body = body.map_or_else(
                    || div().into_any_element(),
                    |screen| {
                        div()
                            .absolute()
                            .left(bounds.origin.x)
                            .top(bounds.origin.y)
                            .w(bounds.size.width)
                            .h(bounds.size.height)
                            .child(screen)
                            .into_any_element()
                    },
                );
                (body, controls.hints, Vec::new())
            }
            (None, Some((body, hints, outline_toolbar))) => {
                toolbar = outline_toolbar;
                (above_pane(body, rows_area_height), hints, Vec::new())
            }
            (Some(mut table), _) => {
                self.editor.viewport.set_height(table_rows);
                self.editor.viewport.margins = (1, 1);
                let top = self.advance_scroll(table_rows, window);
                let first_line = top as usize;
                if first_line != self.editor.viewport.first_line
                    && let Some(shown) = self.editor.table(first_line..first_line + table_rows + 2)
                {
                    table = shown;
                }
                let scrolled = Scrolled {
                    above: top > 0.0,
                    below: table.rows.len() > table_rows + usize::from(top.fract() > 0.0),
                };
                table.rows.truncate(table_rows + 1);
                let buffer = self.editor.window_buffer();
                let cursor = TableCursor {
                    line: buffer.point_line(),
                    column: buffer.point_column(),
                    regions: (first_line..first_line + table.rows.len())
                        .map(|line| self.region_columns(line))
                        .collect(),
                };
                let hints = table.hints;
                let fits = fits_preview(frame_size.width);
                let side = match (table.changes.take(), table.preview.take()) {
                    _ if !fits => None,
                    (Some(changes), _) => Some(render_changes(changes, cx)),
                    (None, preview) => preview.map(|preview| self.render_preview(preview)),
                };
                let body = render_table(table, first_line, scrolled, top.fract(), cursor, side, cx);
                (above_pane(body, rows_area_height), hints, Vec::new())
            }
            (None, None) => {
                let mut controls = self.editor.text_controls().unwrap_or_default();
                let side_width = self.diff.side_width;
                let side = controls
                    .side
                    .take()
                    .filter(|_| fits_beside(frame_size.width, side_width));
                if side.is_some() {
                    self.window_state.text_left = text_side_end(side_width) - px(ROW_MARGIN);
                }
                self.window_state.number_width = controls.number_width;
                let (body, popups) = self.render_text(window, cx);
                let side = side.map(|side| {
                    let side = render_text_side(side, side_width, rows_area_height, cx);
                    above_pane(side, rows_area_height)
                });
                let body = div()
                    .absolute()
                    .inset_0()
                    .child(body)
                    .children(side)
                    .into_any_element();
                if !controls.toolbar.is_empty() {
                    toolbar = Some(render_toolbar(controls.toolbar, cx).into_any_element());
                }
                text_shown = true;
                (body, controls.hints, popups)
            }
        };
        WindowParts {
            body,
            toolbar,
            tabbed,
            hints,
            popups,
            text_shown,
        }
    }
}

/// Lays `layout` out in `bounds`, each window's share rounded to whole pixels, splits'
/// dividers depth first.
pub(super) fn lay_out(
    layout: &Layout,
    bounds: Bounds<Pixels>,
    areas: &mut Vec<(WindowId, Bounds<Pixels>)>,
    dividers: &mut Vec<Divider>,
) {
    match layout {
        Layout::Window(window) => areas.push((*window, bounds)),
        Layout::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let Bounds { origin, size } = bounds;
            let (first_bounds, second_bounds) = match axis {
                Axis::Rows => {
                    let height = (size.height * *ratio).round();
                    (
                        Bounds::new(origin, gpui::size(size.width, height)),
                        Bounds::new(
                            point(origin.x, origin.y + height),
                            gpui::size(size.width, size.height - height),
                        ),
                    )
                }
                Axis::Columns => {
                    let width = (size.width * *ratio).round();
                    (
                        Bounds::new(origin, gpui::size(width, size.height)),
                        Bounds::new(
                            point(origin.x + width, origin.y),
                            gpui::size(size.width - width, size.height),
                        ),
                    )
                }
            };
            let at = match axis {
                Axis::Rows => second_bounds.top(),
                Axis::Columns => second_bounds.left(),
            };
            dividers.push(Divider {
                split: dividers.len(),
                axis: *axis,
                bounds,
                at,
                min: px(0.0),
            });
            lay_out(first, first_bounds, areas, dividers);
            lay_out(second, second_bounds, areas, dividers);
        }
    }
}

/// A 1px line centered in a strip `grab` wide that dragging moves.
/// A line between windows side by side goes on past the windows' area down to where
/// their text ends, `text_bottom`, under the bottom row, where it takes no mouse: the
/// key hints are often over it.
pub(super) fn render_dividers(
    dividers: &[Divider],
    grab: Pixels,
    (area_bottom, text_bottom): (Pixels, Pixels),
    cx: &mut Context<EditorView>,
) -> Vec<Div> {
    let line = || div().bg(rgba(glass::ink(0x14)));
    dividers
        .iter()
        .flat_map(|&divider| {
            let Bounds { origin, size } = divider.bounds;
            let below = (divider.axis == Axis::Columns
                && divider.bounds.bottom() == area_bottom
                && text_bottom > area_bottom)
                .then(|| {
                    line()
                        .absolute()
                        .left(divider.at - px(0.5))
                        .top(area_bottom)
                        .w(px(1.0))
                        .h(text_bottom - area_bottom)
                });
            let line = line();
            let strip = div().absolute().flex().items_center().justify_center();
            let strip = match divider.axis {
                Axis::Rows => strip
                    .left(origin.x)
                    .top(divider.at - grab / 2.0)
                    .w(size.width)
                    .h(grab)
                    .cursor_row_resize()
                    .child(line.w_full().h(px(1.0))),
                Axis::Columns => strip
                    .left(divider.at - grab / 2.0)
                    .top(origin.y)
                    .w(grab)
                    .h(size.height)
                    .cursor_col_resize()
                    .child(line.h_full().w(px(1.0))),
            };
            let strip = strip.occlude().on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    view.divider_drag = Some(divider);
                    cx.notify();
                }),
            );
            iter::once(strip).chain(below)
        })
        .collect()
}

/// In place of a screen another window sizes; a click selects that window.
fn render_shown_elsewhere(owner: WindowId, cx: &mut Context<EditorView>) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .font_family(font::ui())
        .text_size(px(font::ui_size()))
        .text_color(rgb(color::fg_dim()))
        .child("Shown in another window")
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                view.select_clicked(owner, cx);
            }),
        )
        .into_any_element()
}

/// Rows laid out in the top `height` of the window, clear of the pane under them.
fn above_pane(rows: impl IntoElement, height: Pixels) -> AnyElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(height)
        .child(rows)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use gpui::size;
    use remacs_core::Editor;

    use super::*;

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn lays_out_splits_depth_first_in_whole_pixels() {
        let mut editor = Editor::new(&[]);
        editor.split_window(Axis::Columns);
        editor.split_window(Axis::Rows);
        let (mut areas, mut dividers) = (Vec::new(), Vec::new());
        lay_out(
            editor.layout(),
            bounds(0.0, 0.0, 101.0, 50.0),
            &mut areas,
            &mut dividers,
        );
        assert_eq!(areas[0].0, editor.selected_window());
        let areas: Vec<_> = areas.into_iter().map(|(_, area)| area).collect();
        assert_eq!(
            areas,
            [
                bounds(0.0, 0.0, 51.0, 25.0),
                bounds(0.0, 25.0, 51.0, 25.0),
                bounds(51.0, 0.0, 50.0, 50.0),
            ]
        );
        let lines: Vec<_> = dividers
            .iter()
            .map(|divider| (divider.split, divider.axis, divider.at, divider.bounds))
            .collect();
        assert_eq!(
            lines,
            [
                (0, Axis::Columns, px(51.0), bounds(0.0, 0.0, 101.0, 50.0)),
                (1, Axis::Rows, px(25.0), bounds(0.0, 0.0, 51.0, 50.0)),
            ]
        );
    }
}
