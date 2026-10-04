use std::ops::Range;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    AnyElement, ClipboardItem, Context, FocusHandle, HighlightStyle, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, ParentElement, Pixels, Render, ScrollWheelEvent, StyledText,
    Subscription, Window, div, font, linear_color_stop, linear_gradient, prelude::*, px, rgb, rgba,
};
use remacs_core::Editor;
use remacs_theme::{color, font};

use crate::display_line::DisplayLine;
use crate::key_input::to_key;
use crate::minibuffer_view::{render_minibuffer, render_scrim};
use crate::widgets::{cursor_style, hsla, modified_dot};

/// Minified files can hold megabytes on one line; lay out only what can be on screen.
const MAX_RENDERED_LINE_BYTES: usize = 2048;
const TITLE_BAR_HEIGHT: f32 = 44.0;
const HEADER_FADE_HEIGHT: f32 = 80.0;
const FOOTER_FADE_HEIGHT: f32 = 56.0;
const TEXT_TOP: f32 = 64.0;
const ROW_MARGIN: f32 = 8.0;
const GUTTER_PADDING: f32 = 20.0;
const MIN_GUTTER_DIGITS: usize = 3;

pub struct EditorView {
    editor: Editor,
    focus_handle: FocusHandle,
    metrics: Option<Metrics>,
    scroll_remainder: Pixels,
    _activation: Subscription,
}

/// Monospace advances, measured once from the code font.
#[derive(Clone, Copy)]
struct Metrics {
    char_width: Pixels,
    digit_width: Pixels,
}

impl EditorView {
    pub fn new(mut editor: Editor, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (notify, mut notifications) = mpsc::unbounded();
        editor.set_notifier(Arc::new(move || {
            let _ = notify.unbounded_send(());
        }));
        cx.spawn(async move |view, cx| {
            while notifications.next().await.is_some() {
                while notifications.try_recv().is_ok() {}
                let refreshed = view.update(cx, |view, cx| {
                    view.editor.refresh();
                    cx.notify();
                });
                if refreshed.is_err() {
                    break;
                }
            }
        })
        .detach();
        // Another app may have changed the clipboard while this window was in the background.
        let activation = cx.observe_window_activation(window, |view, window, cx| {
            if window.is_window_active() {
                view.receive_clipboard(cx);
            }
        });
        let mut view = Self {
            editor,
            focus_handle: cx.focus_handle(),
            metrics: None,
            scroll_remainder: px(0.0),
            _activation: activation,
        };
        view.receive_clipboard(cx);
        view
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub fn choose_candidate(&mut self, index: usize, cx: &mut Context<Self>) {
        self.editor.choose_candidate(index);
        self.after_input(cx);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = to_key(&event.keystroke) else {
            return;
        };
        self.editor.handle_key(key);
        self.after_input(cx);
    }

    fn after_input(&mut self, cx: &mut Context<Self>) {
        if let Some(killed) = self.editor.kill_ring.take_export() {
            cx.write_to_clipboard(ClipboardItem::new_string(killed));
        }
        if self.editor.quit_requested() {
            cx.quit();
        }
        cx.notify();
    }

    fn receive_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.editor.kill_ring.receive(text);
        }
    }

    fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let line_height = px(font::CODE_LINE_HEIGHT);
        self.scroll_remainder += event.delta.pixel_delta(line_height).y;
        let lines = (self.scroll_remainder / line_height).trunc();
        if lines != 0.0 {
            self.scroll_remainder -= line_height * lines;
            self.editor.scroll(-lines as isize);
            cx.notify();
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        // An open prompt is modal: clicks choose candidates or do nothing.
        let Some(metrics) = self.metrics.filter(|_| self.editor.minibuffer().is_none()) else {
            return;
        };
        let row = ((event.position.y - px(TEXT_TOP)) / px(font::CODE_LINE_HEIGHT)).floor();
        if row < 0.0 {
            return;
        }
        let line = self.editor.viewport.first_line + row as usize;
        let text_left = px(ROW_MARGIN) + self.gutter_width(metrics);
        let display_column = ((event.position.x - text_left) / metrics.char_width)
            .round()
            .max(0.0) as usize;
        let column = self.display_line(line).char_column_at(display_column);
        self.editor.set_point_at(line, column);
        cx.notify();
    }

    fn metrics(&mut self, window: &Window) -> Metrics {
        *self.metrics.get_or_insert_with(|| {
            let text_system = window.text_system();
            let font_id = text_system.resolve_font(&font(font::MONO));
            let advance = |size: f32, character| {
                text_system
                    .advance(font_id, px(size), character)
                    .map_or(px(size * 0.6), |advance| advance.width)
            };
            Metrics {
                char_width: advance(font::CODE_SIZE, 'm'),
                digit_width: advance(font::LINE_NUMBER_SIZE, '0'),
            }
        })
    }

    fn gutter_width(&self, metrics: Metrics) -> Pixels {
        let digits = self
            .editor
            .window_buffer()
            .text()
            .line_count()
            .to_string()
            .len();
        metrics.digit_width * digits.max(MIN_GUTTER_DIGITS) as f32 + px(GUTTER_PADDING)
    }

    fn display_line(&self, line: usize) -> DisplayLine {
        let text = self.editor.window_buffer().text();
        let range = text.line_range(line).unwrap_or_default();
        let visible = range.start..range.end.min(range.start + MAX_RENDERED_LINE_BYTES);
        DisplayLine::new(&String::from_utf8_lossy(&text.bytes_in(visible)))
    }

    /// Characters of `line` inside the active region; past the end means the newline too.
    fn region_columns(&self, line: usize) -> Option<Range<usize>> {
        let buffer = self.editor.window_buffer();
        let region = buffer.region()?;
        let text = buffer.text();
        let line_range = text.line_range(line)?;
        if region.start > line_range.end || region.end <= line_range.start {
            return None;
        }
        let column_of =
            |offset: usize| text.char_column(offset.clamp(line_range.start, line_range.end));
        let end = if region.end > line_range.end {
            column_of(line_range.end) + 1
        } else {
            column_of(region.end)
        };
        Some(column_of(region.start)..end)
    }

    fn render_line(&self, line: usize, gutter_width: Pixels) -> AnyElement {
        let buffer = self.editor.window_buffer();
        let is_current = line == buffer.point_line();
        let mut display_line = self.display_line(line);
        let cursor = is_current.then(|| display_line.cursor_cell(buffer.point_column()));
        let region = self
            .region_columns(line)
            .map(|columns| display_line.cell_range(columns));
        let content =
            StyledText::new(display_line.text).with_highlights(highlights(region, cursor));
        let number_color = if is_current {
            color::FG_MAIN
        } else {
            color::FG_LINE_NUMBER
        };

        div()
            .flex()
            .flex_none()
            .h(px(font::CODE_LINE_HEIGHT))
            .mx(px(ROW_MARGIN))
            .rounded(px(6.0))
            .when(is_current, |row| row.bg(rgb(color::BG_HL_LINE)))
            .whitespace_nowrap()
            .child(
                div()
                    .flex_none()
                    .w(gutter_width)
                    .pr(px(GUTTER_PADDING))
                    .text_right()
                    .text_size(px(font::LINE_NUMBER_SIZE))
                    .text_color(rgb(number_color))
                    .child((line + 1).to_string()),
            )
            .child(div().text_size(px(font::CODE_SIZE)).child(content))
            .into_any_element()
    }

    fn render_header(&self) -> impl IntoElement {
        let buffer = self.editor.window_buffer();
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(HEADER_FADE_HEIGHT))
            .bg(fade(180.0, 0.55))
            .child(
                div()
                    .h(px(TITLE_BAR_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .font_family(font::UI)
                    .text_size(px(font::UI_SIZE))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(buffer.name().to_owned()),
                    )
                    .child(div().text_color(rgb(color::FG_DIM)).child("— remacs")),
            )
    }

    fn render_status(&self) -> impl IntoElement {
        let buffer = self.editor.window_buffer();
        let position = format!("{}:{}", buffer.point_line() + 1, buffer.point_column() + 1);
        let separator = || div().w(px(1.0)).h(px(14.0)).bg(rgba(0xffffff14));

        div()
            .absolute()
            .right(px(16.0))
            .bottom(px(14.0))
            .flex()
            .items_center()
            .gap(px(2.0))
            .p(px(3.0))
            .rounded(px(9.0))
            .bg(rgba(0x1e1e1eb8))
            .border_1()
            .border_color(rgba(0xffffff12))
            .text_size(px(font::STATUS_SIZE))
            .text_color(rgb(color::FG_FAINT))
            .when_some(self.editor.echo(), |status, echo| {
                status
                    .child(div().px(px(8.0)).text_color(rgb(color::FG_ALT)).child(echo))
                    .child(separator())
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .font_family(font::MONO)
                    .when(buffer.is_modified(), |position| {
                        position.child(modified_dot())
                    })
                    .child(position),
            )
            .child(separator())
            .child(
                div()
                    .pl(px(8.0))
                    .pr(px(10.0))
                    .font_family(font::UI)
                    .child(buffer.mode()),
            )
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let metrics = self.metrics(window);
        // Point stays above the footer fade; lines under it are drawn but not counted.
        let line_height = px(font::CODE_LINE_HEIGHT);
        let text_height = window.viewport_size().height - px(TEXT_TOP);
        let clear_rows = ((text_height - px(FOOTER_FADE_HEIGHT)) / line_height).floor();
        let drawn_rows = (text_height / line_height).ceil() as usize;
        self.editor
            .viewport
            .set_height(clear_rows.max(1.0) as usize);
        window.set_window_title(&format!("{} — remacs", self.editor.window_buffer().name()));

        let gutter_width = self.gutter_width(metrics);
        let first_line = self.editor.viewport.first_line;
        let line_count = self.editor.window_buffer().text().line_count();
        let last_line = (first_line + drawn_rows).min(line_count);
        let lines = (first_line..last_line).map(|line| self.render_line(line, gutter_width));
        let minibuffer = self
            .editor
            .minibuffer()
            .map(|minibuffer| render_minibuffer(minibuffer, cx));

        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(rgb(color::BG_MAIN))
            .font_family(font::MONO)
            .text_color(rgb(color::FG_MAIN))
            .line_height(px(font::CODE_LINE_HEIGHT))
            .child(
                div()
                    .absolute()
                    .top(px(TEXT_TOP))
                    .left_0()
                    .right_0()
                    .flex()
                    .flex_col()
                    .children(lines),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(FOOTER_FADE_HEIGHT))
                    .bg(fade(0.0, 0.0)),
            )
            .child(self.render_status())
            .when(minibuffer.is_some(), |root| root.child(render_scrim()))
            .child(self.render_header())
            .children(minibuffer)
    }
}

/// The region and the cursor as sorted, non-overlapping highlights; the cursor wins.
fn highlights(
    region: Option<Range<usize>>,
    cursor: Option<Range<usize>>,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let region_style = HighlightStyle {
        background_color: Some(hsla(color::BG_SELECTION)),
        ..Default::default()
    };
    let mut highlights = Vec::with_capacity(3);
    match (region, cursor) {
        (Some(region), Some(cursor)) => {
            let before = region.start..region.end.min(cursor.start);
            let after = region.start.max(cursor.end)..region.end;
            if !before.is_empty() {
                highlights.push((before, region_style));
            }
            highlights.push((cursor, cursor_style()));
            if !after.is_empty() {
                highlights.push((after, region_style));
            }
        }
        (Some(region), None) => highlights.push((region, region_style)),
        (None, Some(cursor)) => highlights.push((cursor, cursor_style())),
        (None, None) => {}
    }
    highlights
}

/// Black fading to transparent along `angle`, solid until `solid_until` (0..1).
fn fade(angle: f32, solid_until: f32) -> gpui::Background {
    linear_gradient(
        angle,
        linear_color_stop(hsla(color::BG_MAIN), solid_until),
        linear_color_stop(rgba(0x00000000), 1.0),
    )
}
