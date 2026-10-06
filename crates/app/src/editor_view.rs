use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    AnyElement, App, ClipboardItem, Context, DispatchPhase, Div, FocusHandle, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Point, Render, ScrollDelta, ScrollHandle, ScrollWheelEvent, StyledText, Subscription, Task,
    Window, canvas, div, font, prelude::*, px, rgb, rgba,
};
use remacs_core::{
    Action, ChangedLines, Code, DocBlock, Editor, LineChange, MouseSelection, Outline, Popup,
    Preview, PreviewBody, Scroll, StatusSegment,
};
use remacs_theme::font::CodeFont;
use remacs_theme::{color, font, glass};

use crate::changes_view::render_changes;
use crate::conflict_view::codes as conflict_codes;
use crate::display_line::{DisplayLine, display_width, slice_line};
use crate::highlights::{Highlights, combine, face_style, region_style};
use crate::key_input::to_key;
use crate::minibuffer_view::{is_problem_list, render_minibuffer, render_scrim};
use crate::outline_view::{
    COMMIT_HEIGHT, OutlineScrolled, fits_history, fitting_commits, fitting_rows, max_outline_lines,
    render_outline, render_toolbar, row_height,
};
use crate::popup_view::{DocumentationScroll, DocumentationView, Placement, render_popup};
use crate::preview_view::render_preview;
use crate::settings::{CursorHeight, MouseSettings, ReadSettings, ScrollSettings, report};
use crate::smooth_scroll::SmoothScroll;
use crate::table_view::{
    LIST_BOTTOM, ROW_PITCH, ROWS_TOP, TableCursor, fits_preview, render_hints, render_table,
};
use crate::widgets::{
    Edge, SCROLL_FADE_SIZE, Scrolled, cursor_text_style, edge_fade, hsla, icon, modified_dot, pill,
    pill_separator, tone_color,
};

/// Minified files can hold megabytes on one line; lay out only what can be on screen.
const MAX_RENDERED_LINE_BYTES: usize = 2048;
const TITLE_BAR_HEIGHT: f32 = 44.0;
/// Over the status pill; point stays above it.
const FOOTER_FADE_HEIGHT: f32 = 56.0;
const TEXT_TOP: f32 = 64.0;
const ROW_MARGIN: f32 = 8.0;
const ROW_RADIUS: f32 = 6.0;
/// Columns point keeps from a side with text scrolled past it, clear of the side fades;
/// Zed keeps 5, Vim users often set `sidescrolloff` to 5–8.
const SIDE_MARGIN_COLUMNS: usize = 7;
const GUTTER_PADDING: f32 = 20.0;
const MIN_GUTTER_DIGITS: usize = 3;
/// Marks of changed lines are centered this far left of the text.
const CHANGE_MARK_CENTER: f32 = 8.5;
const CHANGE_BAR_WIDTH: f32 = 3.0;
/// Between a bar and the rows above and below it.
const CHANGE_BAR_GAP: f32 = 2.0;
const CHANGE_TICK_WIDTH: f32 = 9.0;
const CHANGE_TICK_HEIGHT: f32 = 3.0;
/// Per mode; one popup shows a handful.
const MAX_HIGHLIGHTED_SNIPPETS: usize = 64;

pub struct EditorView {
    editor: Editor,
    focus_handle: FocusHandle,
    cursor_height: CursorHeight,
    mouse: MouseSettings,
    /// While the button pressed over the text is held.
    mouse_selection: Option<MouseSelection>,
    /// Where the mouse was last seen while selecting.
    drag_position: Point<Pixels>,
    /// Scrolls toward the mouse while it is held past the text's top or bottom.
    drag_scroll: Option<Task<()>>,
    /// And the code font they were measured from.
    metrics: Option<(CodeFont, Metrics)>,
    scroll: ScrollSettings,
    /// Where the lines or rows are drawn, following the viewport's first line.
    vertical: SmoothScroll,
    /// Where the text is drawn sideways, following the viewport's first column.
    sideways: SmoothScroll,
    /// The buffer the scrolls follow; another one is shown at once.
    scrolled_buffer: String,
    /// Commits of an outline's history scrolled past its first one, short of a whole one.
    history_fraction: f32,
    /// Whether the history was last drawn with commits above and below it.
    history_scrolled: Scrolled,
    /// The text was scrolled sideways by hand, so point may be out of view until the next
    /// input, as in VS Code; following point would undo the scroll.
    scrolled_sideways: bool,
    /// Some drawn line goes on past the right edge, so the text can scroll further right.
    more_on_right: bool,
    documentation_scroll: DocumentationScroll,
    /// How far the minibuffer input is scrolled sideways.
    input_scroll: ScrollHandle,
    /// Faces of code shown in popups, by mode and text.
    code_faces: HashMap<&'static str, HashMap<String, Highlights>>,
    /// The window buffer was last drawn as rows, a table's or an outline's, so input maps
    /// to rows, not text.
    showing_rows: bool,
    /// The refresh plugins asked for after a delay, and when it is due.
    refresh_timer: Option<(Instant, Task<()>)>,
    _activation: Subscription,
}

/// Measured from the code font.
#[derive(Clone, Copy)]
struct Metrics {
    char_width: Pixels,
    /// Ascent and descent.
    glyph_height: Pixels,
    digit_width: Pixels,
    /// Of code quoted in notes.
    note_code_width: Pixels,
}

impl EditorView {
    pub fn new(
        mut editor: Editor,
        cursor_height: CursorHeight,
        mouse: MouseSettings,
        scroll: ScrollSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (notify, mut notifications) = mpsc::unbounded();
        editor.set_notifier(Arc::new(move || {
            let _ = notify.unbounded_send(());
        }));
        cx.spawn(async move |view, cx| {
            while notifications.next().await.is_some() {
                while notifications.try_recv().is_ok() {}
                let refreshed = view.update(cx, |view, cx| view.refresh(cx));
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
            cursor_height,
            mouse,
            mouse_selection: None,
            drag_position: Point::default(),
            drag_scroll: None,
            metrics: None,
            scroll,
            vertical: SmoothScroll::default(),
            sideways: SmoothScroll::default(),
            scrolled_buffer: String::new(),
            history_fraction: 0.0,
            history_scrolled: Scrolled::default(),
            scrolled_sideways: false,
            more_on_right: false,
            documentation_scroll: DocumentationScroll::default(),
            input_scroll: ScrollHandle::new(),
            code_faces: HashMap::new(),
            showing_rows: false,
            refresh_timer: None,
            _activation: activation,
        };
        view.receive_clipboard(cx);
        // Lets plugins pick up a file opened from the command line.
        view.editor.refresh();
        view
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// Takes settings read again: the view's part, and the plugins' sections.
    pub fn apply_settings(&mut self, read: ReadSettings, cx: &mut Context<Self>) {
        self.cursor_height = read.ui.cursor.height;
        self.mouse = read.ui.mouse;
        self.scroll = read.ui.scroll;
        self.editor.set_settings(read.settings);
        report(&read.errors, &mut self.editor);
        cx.notify();
    }

    pub fn choose_candidate(&mut self, index: usize, cx: &mut Context<Self>) {
        self.editor.choose_candidate(index);
        self.after_input(cx);
    }

    pub fn focus_candidate(&mut self, index: usize, cx: &mut Context<Self>) {
        self.editor.focus_candidate(index);
        cx.notify();
    }

    pub fn answer_question(&mut self, yes: bool, cx: &mut Context<Self>) {
        self.editor.answer_question(yes);
        self.after_input(cx);
    }

    pub fn toggle_dialog_option(&mut self, cx: &mut Context<Self>) {
        self.editor.toggle_dialog_option();
        cx.notify();
    }

    pub fn toggle_prompt_option(&mut self, index: usize, cx: &mut Context<Self>) {
        self.editor.toggle_prompt_option(index);
        cx.notify();
    }

    pub fn next_prompt_choice(&mut self, index: usize, cx: &mut Context<Self>) {
        self.editor.next_prompt_choice(index);
        cx.notify();
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
        self.scrolled_sideways = false;
        self.schedule_refresh(cx);
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.editor.refresh();
        self.schedule_refresh(cx);
        cx.notify();
    }

    /// Refreshes when a plugin asked to, unless an earlier refresh is already due.
    fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.editor.take_refresh_at() else {
            return;
        };
        if self
            .refresh_timer
            .as_ref()
            .is_some_and(|&(pending, _)| pending <= at)
        {
            return;
        }
        let timer = cx.spawn(async move |view, cx| {
            let delay = at.saturating_duration_since(Instant::now());
            cx.background_executor().timer(delay).await;
            let _ = view.update(cx, |view, cx| {
                view.refresh_timer = None;
                view.refresh(cx);
            });
        });
        self.refresh_timer = Some((at, timer));
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
        let row_height = px(if self.showing_rows {
            ROW_PITCH
        } else {
            font::code().line_height
        });
        let (delta, at_once) = match event.delta {
            // A trackpad moves the content under the fingers, momentum included.
            ScrollDelta::Pixels(delta) => (delta, true),
            ScrollDelta::Lines(_) => (event.delta.pixel_delta(row_height), false),
        };
        // A swipe is never quite straight: it scrolls along its main direction only.
        if delta.x.abs() > delta.y.abs() {
            self.scroll_sideways(-delta.x, at_once, cx);
            return;
        }
        self.scroll_vertically(-delta.y / row_height, at_once);
        self.drag_selection();
        cx.notify();
    }

    /// Scrolls by `rows` lines or rows, fractions included, leaving point where it is
    /// unless it would leave the window; `at_once` skips the glide.
    fn scroll_vertically(&mut self, rows: f32, at_once: bool) {
        let first_line = self.editor.viewport.first_line;
        let last_start = self
            .editor
            .window_buffer()
            .text()
            .line_count()
            .saturating_sub(1);
        let position = (self.vertical.target(first_line) + rows).clamp(0.0, last_start as f32);
        self.editor.scroll(position as isize - first_line as isize);
        let first_line = self.editor.viewport.first_line;
        self.vertical
            .rest_at(first_line, position - first_line as f32, at_once);
    }

    /// Scrolls the text sideways by `delta`, no further right than the drawn lines go,
    /// leaving point where it is.
    fn scroll_sideways(&mut self, delta: Pixels, at_once: bool, cx: &mut Context<Self>) {
        let Some(char_width) = self
            .metrics
            .as_ref()
            .map(|(_, metrics)| metrics.char_width)
            .filter(|_| !self.showing_rows)
        else {
            return;
        };
        let columns = delta / char_width;
        if columns > 0.0 && !self.more_on_right {
            return;
        }
        let viewport = &mut self.editor.viewport;
        let position = (self.sideways.target(viewport.first_column) + columns).max(0.0);
        viewport.first_column = position as usize;
        self.sideways
            .rest_at(viewport.first_column, position.fract(), at_once);
        self.scrolled_sideways = true;
        cx.notify();
    }

    /// Scrolls an outline's history by what the wheel moved, part of a commit included.
    pub fn scroll_history(
        &mut self,
        event: &ScrollWheelEvent,
        scroll: Scroll,
        cx: &mut Context<Self>,
    ) {
        let row_height = px(COMMIT_HEIGHT);
        let delta = event.delta.pixel_delta(row_height).y;
        let mut position = self.history_fraction - delta / row_height;
        // Nothing is above the newest commit or below the oldest.
        if !self.history_scrolled.above {
            position = position.max(0.0);
        }
        if !self.history_scrolled.below {
            position = position.min(0.0);
        }
        let whole = position.floor();
        self.history_fraction = position - whole;
        if whole != 0.0 {
            let rows = whole as isize;
            let action: Action = Rc::new(move |editor| scroll(editor, rows));
            self.run_action(&action, cx);
        } else {
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
        // An open prompt is modal: clicks choose candidates or do nothing. Rows handle
        // their own clicks.
        let Some(metrics) = self
            .metrics
            .as_ref()
            .map(|(_, metrics)| *metrics)
            .filter(|_| self.editor.minibuffer().is_none() && !self.showing_rows)
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
        self.scrolled_sideways = false;
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
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
        // Above the text counts as the first line, which is above any line point is on
        // with the text scrolled.
        let under = self.line_at(self.drag_position.y).unwrap_or(0);
        let line = viewport.clamp(under, line_count);
        let offset = self.offset_at(line, self.drag_position.x, metrics);
        selection.drag(self.editor.window_buffer_mut(), offset);
        self.scrolled_sideways = false;
        // Past the last line there is nothing more to scroll to.
        let last_line = line_count.saturating_sub(1);
        under.min(last_line).cmp(&line.min(last_line)) as isize
    }

    /// The line drawn at `y` in the window, none above the text.
    fn line_at(&self, y: Pixels) -> Option<usize> {
        let rows = (y - px(TEXT_TOP)) / px(font::code().line_height);
        (rows >= 0.0).then(|| (self.vertical.shown() + rows) as usize)
    }

    /// The offset in the window buffer nearest to `x` on `line`; past the last line, the
    /// end of the text.
    fn offset_at(&self, line: usize, x: Pixels, metrics: Metrics) -> usize {
        let text_left = px(ROW_MARGIN) + self.gutter_width(metrics);
        let columns = (x - text_left) / metrics.char_width + self.sideways.shown();
        let display_column = columns.round().max(0.0) as usize;
        let column = self.display_line(line).0.char_column_at(display_column);
        let text = self.editor.window_buffer().text();
        text.offset_at_char_column(line, column)
    }

    /// Follows the mouse anywhere, even outside the window, while a selection is dragged.
    fn render_drag_listeners(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        self.mouse_selection.as_ref()?;
        let view = cx.entity().downgrade();
        let paint = move |_, _, window: &mut Window, _: &mut App| {
            let moved = view.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    let _ = moved.update(cx, |view, cx| view.on_mouse_move(event, cx));
                }
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                    let _ = view.update(cx, |view, cx| {
                        view.release_mouse();
                        cx.notify();
                    });
                }
            });
        };
        Some(canvas(|_, _, _| {}, paint).absolute().size_full())
    }

    fn metrics(&mut self, window: &Window) -> Metrics {
        let code_font = font::code();
        if let Some((measured, metrics)) = &self.metrics
            && *measured == code_font
        {
            return *metrics;
        }
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&font(code_font.family.clone()));
        let advance = |size: f32, character| {
            text_system
                .advance(font_id, px(size), character)
                .map_or(px(size * 0.6), |advance| advance.width)
        };
        let size = px(code_font.size);
        let metrics = Metrics {
            char_width: advance(code_font.size, 'm'),
            // The font's descent is negative here, unlike a laid-out line's.
            glyph_height: text_system.ascent(font_id, size)
                + text_system.descent(font_id, size).abs(),
            digit_width: advance(code_font.line_number_size(), '0'),
            note_code_width: advance(font::STATUS_SIZE, 'm'),
        };
        self.metrics = Some((code_font, metrics));
        metrics
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

    /// The part of `line` in the drawn columns, and the bytes of the buffer it shows.
    fn display_line(&self, line: usize) -> (DisplayLine, Range<usize>) {
        let text = self.editor.window_buffer().text();
        let range = text.line_range(line).unwrap_or_default();
        let first_column = self.first_drawn_column();
        // One more for the column partly scrolled in.
        let columns = first_column..first_column + self.editor.viewport.width + 1;
        let part = slice_line(text.chunks(range.clone()).flatten().copied(), columns);
        let start = range.start + part.bytes.start;
        let end = range.start + part.bytes.end;
        let visible = start..end.min(start + MAX_RENDERED_LINE_BYTES);
        let continues = part.continues || visible.end < end;
        let source = text.bytes_in(visible.clone());
        let source = String::from_utf8_lossy(&source);
        (DisplayLine::part(&source, part.start, continues), visible)
    }

    /// The display column at the left edge, partly scrolled past.
    fn first_drawn_column(&self) -> usize {
        self.sideways.shown() as usize
    }

    /// The display column of the window buffer's point.
    fn point_display_column(&self) -> usize {
        let buffer = self.editor.window_buffer();
        let text = buffer.text();
        let line_start = text
            .line_range(buffer.point_line())
            .unwrap_or_default()
            .start;
        display_width(text.chunks(line_start..buffer.point()).flatten().copied())
    }

    /// Each decorator's faces on the `visible` bytes, mapped onto their display text.
    fn face_layers(&self, visible: Range<usize>, display_line: &DisplayLine) -> Vec<Highlights> {
        let to_line = |offset: usize| offset.clamp(visible.start, visible.end) - visible.start;
        self.editor
            .decorations(visible.clone())
            .into_iter()
            .map(|spans| {
                spans
                    .into_iter()
                    .map(|span| {
                        let range = to_line(span.range.start)..to_line(span.range.end);
                        (display_line.display_range(range), face_style(span.face))
                    })
                    .collect()
            })
            .collect()
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

    /// The row of `line`, and whether the line goes on past its right edge.
    fn render_line(
        &self,
        line: usize,
        metrics: Metrics,
        gutter_width: Pixels,
    ) -> (AnyElement, bool) {
        let code_font = font::code();
        let row_height = px(code_font.line_height);
        let buffer = self.editor.window_buffer();
        let is_current = line == buffer.point_line();
        let (mut display_line, visible) = self.display_line(line);
        let mut layers = self.face_layers(visible, &display_line);
        if let Some(columns) = self.region_columns(line) {
            layers.push(vec![(display_line.cell_range(columns), region_style())]);
        }
        let mut cursor = None;
        if is_current {
            let cell = display_line.cursor_cell(buffer.point_column());
            let column = display_line.text[..cell.start].chars().count();
            layers.push(vec![(cell, cursor_text_style())]);
            let height = match self.cursor_height {
                CursorHeight::Line => row_height,
                CursorHeight::Font => metrics.glyph_height.min(row_height),
            };
            cursor = Some(
                div()
                    .absolute()
                    .left(metrics.char_width * column as f32)
                    .top((row_height - height) / 2.0)
                    .w(metrics.char_width)
                    .h(height)
                    .bg(rgb(color::FG_MAIN)),
            );
        }
        let scrolled_past = self.sideways.shown() - display_line.first_column() as f32;
        let (hidden_start, continues) = (display_line.has_hidden_start(), display_line.continues);
        let content = StyledText::new(display_line.text).with_highlights(combine(&layers));
        let number_color = if is_current {
            color::FG_MAIN
        } else {
            color::FG_LINE_NUMBER
        };
        // The text fades into what the row is drawn on.
        let under = if is_current {
            hsla(color::BG_HL_LINE)
        } else {
            rgba(glass::WINDOW_TINT).into()
        };

        let row = div()
            .flex()
            .flex_none()
            .h(row_height)
            .mx(px(ROW_MARGIN))
            .rounded(px(ROW_RADIUS))
            .when(is_current, |row| row.bg(rgb(color::BG_HL_LINE)))
            .whitespace_nowrap()
            .line_height(row_height)
            .child(
                div()
                    .flex_none()
                    .w(gutter_width)
                    .pr(px(GUTTER_PADDING))
                    .text_right()
                    .text_size(px(code_font.line_number_size()))
                    .text_color(rgb(number_color))
                    .child((line + 1).to_string()),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    // Clear of the row's rounded end, which a fade would fill.
                    .mr(px(ROW_RADIUS))
                    .overflow_hidden()
                    .child(
                        div()
                            .relative()
                            .left(-metrics.char_width * scrolled_past.max(0.0))
                            .text_size(px(code_font.size))
                            // Before the text, so that it is drawn under it.
                            .children(cursor)
                            .child(content),
                    )
                    .when(hidden_start, |text| {
                        text.child(edge_fade(Edge::Left, SCROLL_FADE_SIZE, under))
                    })
                    .when(continues, |text| {
                        text.child(edge_fade(Edge::Right, SCROLL_FADE_SIZE, under))
                    }),
            );
        (row.into_any_element(), continues)
    }

    /// Popups next to the text they belong to, e.g. completions under the word or the
    /// documentation of a symbol.
    fn render_popups(
        &mut self,
        metrics: Metrics,
        gutter_width: Pixels,
        window: &Window,
    ) -> Vec<AnyElement> {
        let window_size = window.viewport_size();
        let line_height = px(font::code().line_height);
        let popups = self.editor.popups();
        self.documentation_scroll.update(&popups);
        self.highlight_code_in(&popups);
        let code_faces = |code: &Code| self.faces_of(code);
        let view = DocumentationView {
            code_faces: &code_faces,
            scroll: &self.documentation_scroll.handle,
            note_code_width: metrics.note_code_width,
        };
        popups
            .iter()
            .filter_map(|popup| {
                let line = self.editor.window_buffer().text().line_of(popup.anchor);
                let row = line as f32 - self.vertical.shown();
                if row < 0.0 {
                    return None;
                }
                let line_top = px(TEXT_TOP) + line_height * row;
                if line_top >= window_size.height {
                    return None;
                }
                let (display_line, visible) = self.display_line(line);
                let offset = popup.anchor.clamp(visible.start, visible.end) - visible.start;
                let byte = display_line.display_range(offset..offset).start;
                let column = (display_line.first_column()
                    + display_line.text[..byte].chars().count())
                    as f32
                    - self.sideways.shown();
                let anchor_left =
                    px(ROW_MARGIN) + gutter_width + metrics.char_width * column.max(0.0);
                let placement = Placement {
                    anchor_left,
                    line_top,
                    line_bottom: line_top + line_height,
                    area_top: px(TITLE_BAR_HEIGHT),
                    window_width: window_size.width,
                    window_height: window_size.height,
                };
                Some(render_popup(popup, placement, &view))
            })
            .collect()
    }

    /// Faces for the code in the popups' documentation.
    fn highlight_code_in(&mut self, popups: &[Popup]) {
        let codes = popups
            .iter()
            .filter_map(|popup| popup.documentation.as_ref())
            .flat_map(|documentation| {
                let blocks = documentation.blocks.iter().filter_map(|block| match block {
                    DocBlock::Code(code) => Some(code),
                    _ => None,
                });
                documentation.signature.iter().chain(blocks)
            });
        for code in codes {
            self.highlight_code(code);
        }
    }

    /// Parses `code` for [`Self::faces_of`] unless it already was.
    fn highlight_code(&mut self, code: &Code) {
        let Some(mode) = code.mode else {
            return;
        };
        let known = self.code_faces.entry(mode).or_default();
        if known.contains_key(&code.text) {
            return;
        }
        if known.len() >= MAX_HIGHLIGHTED_SNIPPETS {
            known.clear();
        }
        let faces = self
            .editor
            .highlight_code(mode, &code.text)
            .into_iter()
            .map(|span| (span.range, face_style(span.face)))
            .collect();
        known.insert(code.text.clone(), faces);
    }

    fn faces_of(&self, code: &Code) -> Highlights {
        code.mode
            .and_then(|mode| self.code_faces.get(mode)?.get(&code.text))
            .cloned()
            .unwrap_or_default()
    }

    fn render_preview(&mut self, preview: Preview) -> Div {
        if let PreviewBody::Code(code) = &preview.body {
            self.highlight_code(code);
        }
        render_preview(preview, &|code| self.faces_of(code))
    }

    fn render_header(&self) -> Div {
        let buffer = self.editor.window_buffer();
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
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
            .child(div().text_color(rgb(color::FG_DIM)).child("— remacs"))
    }

    /// The window buffer's lines from the viewport on, with the popups placed over them.
    fn render_text(&mut self, window: &Window) -> (AnyElement, Vec<AnyElement>) {
        let metrics = self.metrics(window);
        // Point stays above the footer fade; lines under it are drawn but not counted.
        let line_height = px(font::code().line_height);
        let text_height = window.viewport_size().height - px(TEXT_TOP);
        let clear_rows = ((text_height - px(FOOTER_FADE_HEIGHT)) / line_height)
            .floor()
            .max(1.0) as usize;
        let drawn_rows = (text_height / line_height).ceil() as usize;
        self.editor.viewport.set_height(clear_rows);
        // Only the top fade is over lines point can be on.
        self.editor.viewport.margins = (1, 0);
        let gutter_width = self.gutter_width(metrics);
        let text_width =
            window.viewport_size().width - px(2.0 * ROW_MARGIN + ROW_RADIUS) - gutter_width;
        self.editor.viewport.width = (text_width / metrics.char_width).floor().max(1.0) as usize;
        self.editor.viewport.column_margin = SIDE_MARGIN_COLUMNS;
        if !self.scrolled_sideways {
            let point_column = self.point_display_column();
            self.editor.viewport.ensure_column_visible(point_column);
        }

        let top = self.advance_scroll(clear_rows, window);
        let first_line = top as usize;
        let line_count = self.editor.window_buffer().text().line_count();
        // One more for the line partly scrolled in at the bottom.
        let last_line = (first_line + drawn_rows + 1).min(line_count);
        let popups = self.render_popups(metrics, gutter_width, window);
        let (lines, continued): (Vec<_>, Vec<_>) = (first_line..last_line)
            .map(|line| self.render_line(line, metrics, gutter_width))
            .unzip();
        self.more_on_right = continued.contains(&true);
        let text_left = px(ROW_MARGIN) + gutter_width;
        let changes = self
            .editor
            .line_changes(first_line..last_line)
            .into_iter()
            .filter_map(|changed| render_line_change(changed, first_line..last_line, text_left));
        let glass = rgba(glass::WINDOW_TINT).into();
        let lines = div()
            .relative()
            .top(-line_height * top.fract())
            .flex()
            .flex_col()
            .children(lines)
            .children(changes);
        let body = div()
            .absolute()
            .top(px(TEXT_TOP))
            .left_0()
            .right_0()
            .bottom_0()
            .overflow_hidden()
            .child(lines)
            .when(top > 0.0, |body| {
                // Over the top margin's line.
                body.child(edge_fade(Edge::Top, line_height.into(), glass))
            })
            .when(first_line + clear_rows < line_count, |body| {
                body.child(edge_fade(Edge::Bottom, FOOTER_FADE_HEIGHT, glass))
            })
            .into_any_element();
        (body, popups)
    }

    /// Moves where the content is drawn toward the viewport, `screen_rows` tall, and
    /// returns where it is drawn from; another buffer is shown at once. Asks for frames
    /// while it glides.
    fn advance_scroll(&mut self, screen_rows: usize, window: &Window) -> f32 {
        let viewport = self.editor.viewport;
        let name = self.editor.window_buffer().name();
        if self.scrolled_buffer != name {
            self.scrolled_buffer = name.to_owned();
            self.vertical.jump(viewport.first_line);
            self.sideways.jump(viewport.first_column);
            self.history_fraction = 0.0;
        }
        let now = Instant::now();
        let duration = self.scroll.animation_duration();
        let top = self
            .vertical
            .advance(viewport.first_line, now, duration, screen_rows as f32);
        self.sideways
            .advance(viewport.first_column, now, duration, viewport.width as f32);
        if self.vertical.is_animating() || self.sideways.is_animating() {
            window.request_animation_frame();
        }
        top
    }

    pub fn run_command(&mut self, name: &str, cx: &mut Context<Self>) {
        self.editor.run_command(name);
        self.after_input(cx);
    }

    pub fn run_action(&mut self, action: &Action, cx: &mut Context<Self>) {
        if self.editor.minibuffer().is_none() {
            self.editor.run_action(action);
            self.after_input(cx);
        }
    }

    /// Selects a table row; a double click also runs the table's `activate` command.
    pub fn click_row(
        &mut self,
        line: usize,
        click_count: usize,
        activate: &'static str,
        cx: &mut Context<Self>,
    ) {
        if self.editor.minibuffer().is_some() {
            return;
        }
        self.editor.set_point_at(line, 0);
        if click_count == 2 {
            self.editor.run_command(activate);
        }
        self.after_input(cx);
    }

    /// Selects an outline's heading and runs `toggle`, folding or unfolding it.
    pub fn click_heading(&mut self, line: usize, toggle: &'static str, cx: &mut Context<Self>) {
        if self.editor.minibuffer().is_some() {
            return;
        }
        self.editor.set_point_at(line, 0);
        self.editor.run_command(toggle);
        self.after_input(cx);
    }

    /// The window buffer's outline in the rows that fit, the viewport set to them.
    fn render_outline(
        &mut self,
        outline_lines: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, AnyElement, Option<AnyElement>)> {
        let height = window.viewport_size().height;
        let mut outline = self.editor.outline(
            self.editor.viewport.first_line..self.editor.viewport.first_line + outline_lines,
        )?;
        let fit = |outline: &Outline| {
            let has_banner = outline.banner.is_some();
            fitting_rows(&outline.rows, outline.facts.len(), has_banner, height)
        };
        let mut fitting = fit(&outline);
        let point_line = self.editor.window_buffer().point_line();
        let shown_from = self.editor.viewport.first_line;
        self.editor.viewport.set_height(fitting);
        self.editor.viewport.margins = (1, 1);
        self.editor.show_point();
        if self.editor.viewport.first_line != shown_from {
            let first_line = self.editor.viewport.first_line;
            outline = self
                .editor
                .outline(first_line..first_line + outline_lines)?;
            fitting = fit(&outline);
        }
        self.editor.viewport.set_height(fitting);
        let top = self.advance_scroll(fitting, window);
        let first_line = top as usize;
        if first_line != self.editor.viewport.first_line {
            outline = self
                .editor
                .outline(first_line..first_line + outline_lines)?;
            fitting = fit(&outline);
        }
        let mut scrolled = OutlineScrolled {
            rows: Scrolled {
                above: top > 0.0,
                below: outline.rows.len() > fitting,
            },
            rows_shift: row_height(&outline.rows, 0) * top.fract(),
            history: Scrolled::default(),
            history_shift: 0.0,
        };
        // One more for the row partly scrolled in at the bottom.
        outline.rows.truncate(fitting + 1);
        if !fits_history(window.viewport_size().width) {
            outline.history = None;
            outline.conflict = None;
        }
        let has_banner = outline.banner.is_some();
        if let Some(history) = &mut outline.history {
            let fitting = fitting_commits(height, has_banner);
            self.history_scrolled = Scrolled {
                above: history.scrolled,
                below: history.commits.len() > fitting,
            };
            if !self.history_scrolled.below {
                self.history_fraction = 0.0;
            }
            scrolled.history = Scrolled {
                above: history.scrolled || self.history_fraction > 0.0,
                ..self.history_scrolled
            };
            scrolled.history_shift = COMMIT_HEIGHT * self.history_fraction;
            history.commits.truncate(fitting + 1);
        }
        for code in outline.conflict.iter().flat_map(conflict_codes) {
            self.highlight_code(&code);
        }
        let hints = render_hints(outline.hints).into_any_element();
        let toolbar = std::mem::take(&mut outline.toolbar);
        let toolbar = (!toolbar.is_empty()).then(|| render_toolbar(toolbar, cx).into_any_element());
        let faces = |code: &Code| self.faces_of(code);
        let body = render_outline(outline, first_line, point_line, scrolled, &faces, cx)
            .into_any_element();
        Some((body, hints, toolbar))
    }

    /// A plugin's group of the status pill, e.g. problem counts; a click runs its command.
    /// One without a command is a plain label, like the mode.
    fn render_segment(segment: StatusSegment, cx: &mut Context<Self>) -> impl IntoElement {
        let text_color = match (segment.active, segment.command, segment.code) {
            (true, ..) => color::FG_MAIN,
            (false, Some(_), _) | (false, None, true) => color::FG_KBD,
            (false, None, false) => color::FG_FAINT,
        };
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .h(px(24.0))
            .px(px(8.0))
            .rounded(px(6.0))
            .when(segment.active, |button| button.bg(rgba(glass::SELECTION)))
            .font_family(if segment.code {
                font::mono()
            } else {
                font::UI.into()
            })
            .text_color(rgb(text_color))
            .when_some(segment.command, |button, command| {
                button.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        view.run_command(command, cx);
                    }),
                )
            })
            .children(segment.items.into_iter().map(|item| {
                let item_color = tone_color(item.tone, text_color);
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .text_color(rgb(item_color))
                    .children(item.icon.map(|item_icon| icon(item_icon, item_color)))
                    .child(item.text)
            }))
    }

    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let buffer = self.editor.window_buffer();
        let position = format!("{}:{}", buffer.point_line() + 1, buffer.point_column() + 1);
        let segments = self
            .editor
            .status_segments()
            .into_iter()
            .flat_map(|segment| {
                [
                    Self::render_segment(segment, cx).into_any_element(),
                    pill_separator().into_any_element(),
                ]
            })
            .collect::<Vec<_>>();

        pill()
            .right(px(16.0))
            .when_some(self.editor.echo(), |status, echo| {
                status
                    .child(div().px(px(8.0)).text_color(rgb(color::FG_ALT)).child(echo))
                    .child(pill_separator())
            })
            .children(segments)
            // The selected row is the position.
            .when(!self.showing_rows, |status| {
                status
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .px(px(8.0))
                            .font_family(font::mono())
                            .when(buffer.is_modified(), |position| {
                                position.child(modified_dot())
                            })
                            .child(position),
                    )
                    .child(pill_separator())
            })
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
        window.set_window_title(&format!("{} — remacs", self.editor.window_buffer().name()));
        let first_line = self.editor.viewport.first_line;
        let table_rows = ((window.viewport_size().height - px(ROWS_TOP) - px(LIST_BOTTOM))
            / px(ROW_PITCH))
        .floor()
        .max(1.0) as usize;
        // One row more than fits is partly scrolled in; another tells whether there are
        // rows below.
        let table = self.editor.table(first_line..first_line + table_rows + 2);
        let outline = match table {
            Some(_) => None,
            None => {
                let lines = max_outline_lines(window.viewport_size().height);
                self.render_outline(lines, window, cx)
            }
        };
        self.showing_rows = table.is_some() || outline.is_some();
        let mut toolbar = None;
        let (body, hints, popups) = match (table, outline) {
            (None, Some((body, hints, outline_toolbar))) => {
                toolbar = outline_toolbar;
                (body, Some(hints), Vec::new())
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
                let hints = render_hints(table.hints).into_any_element();
                let fits = fits_preview(window.viewport_size().width);
                let side = match (table.changes.take(), table.preview.take()) {
                    _ if !fits => None,
                    (Some(changes), _) => Some(render_changes(changes, cx)),
                    (None, preview) => preview.map(|preview| self.render_preview(preview)),
                };
                let body = render_table(table, first_line, scrolled, top.fract(), cursor, side, cx)
                    .into_any_element();
                (body, Some(hints), Vec::new())
            }
            (None, None) => {
                let (body, popups) = self.render_text(window);
                (body, None, popups)
            }
        };
        let minibuffer = self
            .editor
            .minibuffer()
            .map(|minibuffer| render_minibuffer(minibuffer, &self.input_scroll, cx));
        // gpui scrolls the input to the cursor only once it has laid it out.
        if minibuffer.is_some() && self.input_scroll.bounds().size.width == px(0.0) {
            window.request_animation_frame();
        }
        // The pill stays lit above the scrim while the list it opened is shown.
        let status = self.render_status(cx).into_any_element();
        let (status_below_scrim, status_above_scrim) =
            if self.editor.minibuffer().is_some_and(is_problem_list) {
                (None, Some(status))
            } else {
                (Some(status), None)
            };
        let modal = self
            .editor
            .minibuffer()
            .is_some_and(|minibuffer| minibuffer.dialog().is_some());

        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(rgba(glass::WINDOW_TINT))
            .font_family(font::mono())
            .text_color(rgb(color::FG_MAIN))
            .line_height(px(font::LINE_HEIGHT))
            .child(body)
            .children(hints)
            .children(status_below_scrim)
            .when(minibuffer.is_some(), |root| root.child(render_scrim(modal)))
            .child(self.render_header().children(toolbar))
            .children(popups)
            .children(status_above_scrim)
            .children(minibuffer)
            .children(self.render_drag_listeners(cx))
    }
}

/// A bar in the gutter beside added or modified lines, or a tick between the lines where
/// some were removed; `shown` are the drawn lines, `text_left` where their text starts.
fn render_line_change(
    changed: ChangedLines,
    shown: Range<usize>,
    text_left: Pixels,
) -> Option<AnyElement> {
    let line_height = px(font::code().line_height);
    let top = |line: usize| line_height * (line - shown.start) as f32;
    let mark = if changed.lines.is_empty() {
        div()
            .top(top(changed.lines.start) - px(CHANGE_TICK_HEIGHT / 2.0))
            .left(text_left - px(CHANGE_MARK_CENTER + CHANGE_TICK_WIDTH / 2.0))
            .w(px(CHANGE_TICK_WIDTH))
            .h(px(CHANGE_TICK_HEIGHT))
    } else {
        let lines = changed.lines.start.max(shown.start)..changed.lines.end.min(shown.end);
        if lines.is_empty() {
            return None;
        }
        div()
            .top(top(lines.start) + px(CHANGE_BAR_GAP))
            .left(text_left - px(CHANGE_MARK_CENTER + CHANGE_BAR_WIDTH / 2.0))
            .w(px(CHANGE_BAR_WIDTH))
            .h(line_height * lines.len() as f32 - px(2.0 * CHANGE_BAR_GAP))
    };
    let color = match changed.change {
        LineChange::Added => color::VC_ADDED_MARK,
        LineChange::Modified => color::VC_MODIFIED_MARK,
        LineChange::Removed => color::VC_REMOVED_MARK,
    };
    Some(
        mark.absolute()
            .rounded(px(2.0))
            .bg(rgba(color))
            .into_any_element(),
    )
}
