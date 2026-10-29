use std::time::Instant;

use gpui::{
    AnyElement, Bounds, Context, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point,
    ScrollWheelEvent, SharedString, Size, Window, div, point, prelude::*, px, size,
};
use remacs_core::{PaneButton, ScreenButton, ScreenMouse, ScreenPointer, ScreenSize, Tabs};
use remacs_theme::font;

use super::text::cached_metrics;
use super::{EditorView, TITLE_BAR_HEIGHT};
use crate::key_input::to_modifiers;
use crate::pane_view::render_button;
use crate::screen_view::{Cell as ScreenCell, cell_at, render_screen, screen_size};
use crate::tabs_view::render_tabs;

/// Around the window buffer's screen: its sides, and under it, over the hints and pill.
const SCREEN_SIDE: f32 = 24.0;
const SCREEN_BOTTOM: f32 = 64.0;
/// Where chips in the title bar start, after the window's buttons.
const HEADER_TABS_LEFT: f32 = 92.0;
/// Where chips start in the title bar of a window clear of the frame's buttons.
const HEADER_TABS_SIDE: f32 = 16.0;
/// Kept clear of chips on the title bar's right, for the terminal's one button.
const HEADER_BUTTONS_WIDTH: f32 = 200.0;

/// Where a screen's cells are drawn, to map the mouse to them.
#[derive(Clone)]
pub(super) struct ScreenArea {
    pub(super) buffer: String,
    origin: Point<Pixels>,
    cell: ScreenCell,
    size: ScreenSize,
}

impl ScreenArea {
    /// The mouse at `position`, over the cell under it or the nearest one.
    pub(super) fn pointer(
        &self,
        position: Point<Pixels>,
        modifiers: &gpui::Modifiers,
        mouse: ScreenMouse,
    ) -> ScreenPointer {
        let (row, column) = cell_at(
            (position.x - self.origin.x).max(px(0.0)),
            (position.y - self.origin.y).max(px(0.0)),
            self.cell,
        );
        ScreenPointer {
            row: row.min(self.size.rows.saturating_sub(1)),
            column: column.min(self.size.columns.saturating_sub(1)),
            modifiers: to_modifiers(modifiers),
            mouse,
        }
    }
}

pub(super) fn screen_button(button: MouseButton) -> ScreenButton {
    match button {
        MouseButton::Right => ScreenButton::Right,
        MouseButton::Middle => ScreenButton::Middle,
        _ => ScreenButton::Left,
    }
}

/// Where the window buffer's screen is drawn in a window laid out in `frame`, clear of the
/// title bar, and of the pill or the pane under it.
pub(super) fn full_screen_bounds(frame: Size<Pixels>, reserved: Pixels) -> Bounds<Pixels> {
    let origin = point(px(SCREEN_SIDE), px(TITLE_BAR_HEIGHT));
    Bounds {
        origin,
        size: size(
            frame.width - px(2.0 * SCREEN_SIDE),
            frame.height - origin.y - px(SCREEN_BOTTOM).max(reserved),
        ),
    }
}

impl EditorView {
    /// The buffer named `name` drawn as a screen in `bounds` over `under`, taking the mouse
    /// over it; `bounds` are within what is drawn at `offset` in the frame.
    pub(super) fn render_screen_in(
        &mut self,
        name: &str,
        bounds: Bounds<Pixels>,
        offset: Point<Pixels>,
        under: Hsla,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let code_font = font::code();
        let metrics = cached_metrics(&mut self.screen_metrics, code_font.clone(), window);
        let cell = ScreenCell {
            width: metrics.char_width,
            height: px(code_font.line_height),
            glyph: metrics.glyph_height,
        };
        let padding = self.editor.screen_padding(name);
        let inset = point(px(padding.horizontal), px(padding.vertical));
        let room = size(
            (bounds.size.width - inset.x * 2.0).max(px(0.0)),
            (bounds.size.height - inset.y * 2.0).max(px(0.0)),
        );
        let size = screen_size(room.width, room.height, cell);
        let screen = self.editor.screen(name, size)?;
        self.screen_drawn = true;
        let transition = self.transitions.popup();
        let tip = &mut self.surfaces.mark_tip;
        tip.update(self.hovered_mark, Instant::now(), transition.duration);
        let tip = tip
            .shown(transition)
            .map(|shown| (*shown.content, shown.look));
        let area = ScreenArea {
            buffer: name.to_owned(),
            origin: offset + bounds.origin + inset,
            cell,
            size,
        };
        let mut element = div()
            .id(SharedString::from(format!("screen {name}")))
            .size_full()
            .child(render_screen(screen, cell, inset, under, tip, cx));
        for button in [MouseButton::Left, MouseButton::Middle, MouseButton::Right] {
            let area = area.clone();
            element = element.on_mouse_down(
                button,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&view.focus_handle);
                    view.press_screen(area.clone(), event, cx);
                }),
            );
        }
        let (moved, left, scrolled) = (area.clone(), area.clone(), area);
        let element = element
            .on_mouse_move(cx.listener(move |view, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button.is_none() {
                    view.point_at_screen(&moved, event.position, &event.modifiers, cx);
                }
            }))
            .on_hover(cx.listener(move |view, hovered: &bool, window, cx| {
                if !hovered {
                    let modifiers = window.modifiers();
                    let pointer = left.pointer(left.origin, &modifiers, ScreenMouse::Leave);
                    if view.editor.screen_mouse(&left.buffer, pointer) {
                        cx.notify();
                    }
                }
            }))
            .on_scroll_wheel(cx.listener(move |view, event: &ScrollWheelEvent, _, cx| {
                cx.stop_propagation();
                view.scroll_screen(&scrolled, event, cx);
            }));
        Some(element.into_any_element())
    }

    fn press_screen(&mut self, area: ScreenArea, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let mouse = ScreenMouse::Press {
            button: screen_button(event.button),
            clicks: event.click_count,
        };
        let pointer = area.pointer(event.position, &event.modifiers, mouse);
        self.editor.screen_mouse(&area.buffer, pointer);
        self.screen_press = Some((area, event.button));
        self.after_input(cx);
    }

    /// The mouse moved over a screen with no button held.
    fn point_at_screen(
        &mut self,
        area: &ScreenArea,
        position: Point<Pixels>,
        modifiers: &gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let pointer = area.pointer(position, modifiers, ScreenMouse::Move);
        if self.editor.screen_mouse(&area.buffer, pointer) {
            cx.notify();
        }
    }

    fn scroll_screen(
        &mut self,
        area: &ScreenArea,
        event: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) {
        let row_height = area.cell.height;
        let delta = event.delta.pixel_delta(row_height);
        // macOS turns a mouse's wheel sideways while shift is held.
        let delta = if event.modifiers.shift && delta.y == px(0.0) {
            delta.x
        } else {
            delta.y
        };
        let rows = self.screen_scroll - delta / row_height;
        let whole = rows.trunc();
        self.screen_scroll = rows - whole;
        if whole != 0.0 {
            let mouse = ScreenMouse::Wheel {
                rows: whole as isize,
            };
            let pointer = area.pointer(event.position, &event.modifiers, mouse);
            self.editor.screen_mouse(&area.buffer, pointer);
        }
        cx.notify();
    }

    pub fn hover_command_mark(&mut self, mark: Option<(usize, bool)>, cx: &mut Context<Self>) {
        if mark.is_some() || self.hovered_mark.is_some() {
            self.hovered_mark = mark;
            cx.notify();
        }
    }

    /// Chips on the left of the title bar and buttons on its right, for a screen.
    pub(super) fn render_header_controls(
        &self,
        tabs: Option<Tabs>,
        buttons: Vec<PaneButton>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if tabs.is_none() && buttons.is_empty() {
            return None;
        }
        let clip = self.window_state.clip;
        // Only the window in the top left corner has macOS's buttons in its title bar;
        // elsewhere the desktop draws its own above the window.
        let tabs_left = if cfg!(target_os = "macos") && clip.origin == Point::default() {
            HEADER_TABS_LEFT
        } else {
            HEADER_TABS_SIDE
        };
        let room = f32::from(clip.size.width) - tabs_left - HEADER_BUTTONS_WIDTH;
        let controls = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(TITLE_BAR_HEIGHT))
            .children(tabs.map(|tabs| {
                render_tabs(tabs, room, cx)
                    .absolute()
                    .top(px(9.0))
                    .left(px(tabs_left))
            }))
            .child(
                div()
                    .absolute()
                    .top(px(8.0))
                    .right(px(14.0))
                    .flex()
                    .gap(px(6.0))
                    .font_family(font::ui())
                    .text_size(px(font::status_size()))
                    .children(buttons.into_iter().map(|button| render_button(button, cx))),
            );
        Some(controls.into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn full_screen_clears_the_title_bar_and_the_larger_of_pill_and_pane() {
        let frame = size(px(800.0), px(600.0));
        assert_eq!(
            full_screen_bounds(frame, px(0.0)),
            bounds(
                SCREEN_SIDE,
                TITLE_BAR_HEIGHT,
                752.0,
                600.0 - TITLE_BAR_HEIGHT - SCREEN_BOTTOM
            )
        );
        assert_eq!(
            full_screen_bounds(frame, px(100.0)).size.height,
            px(600.0 - TITLE_BAR_HEIGHT - 100.0)
        );
    }

    #[test]
    fn screen_pointer_takes_the_nearest_cell() {
        let area = ScreenArea {
            buffer: "screen".into(),
            origin: point(px(10.0), px(20.0)),
            cell: ScreenCell {
                width: px(8.0),
                height: px(16.0),
                glyph: px(12.0),
            },
            size: ScreenSize {
                columns: 10,
                rows: 5,
            },
        };
        let mouse = ScreenMouse::Press {
            button: screen_button(MouseButton::Left),
            clicks: 1,
        };
        let cell = |x: f32, y: f32| {
            let pointer = area.pointer(point(px(x), px(y)), &gpui::Modifiers::default(), mouse);
            (pointer.row, pointer.column)
        };
        assert_eq!(cell(0.0, 0.0), (0, 0));
        assert_eq!(
            cell(10.0 + 8.0 * 3.0 + 1.0, 20.0 + 16.0 * 2.0 + 1.0),
            (2, 3)
        );
        assert_eq!(cell(1000.0, 1000.0), (4, 9));
    }
}
