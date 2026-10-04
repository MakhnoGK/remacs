use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    AnyElement, ClipboardItem, Context, Div, FocusHandle, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, ParentElement, Pixels, Render, ScrollHandle, ScrollWheelEvent, StyledText,
    Subscription, Window, div, font, linear_color_stop, linear_gradient, prelude::*, px, rgb, rgba,
};
use remacs_core::{
    Action, Code, DocBlock, Editor, Popup, Preview, PreviewBody, StatusSegment, Tone,
};
use remacs_theme::{color, font};

use crate::changes_view::render_changes;
use crate::display_line::DisplayLine;
use crate::highlights::{Highlights, combine, face_style, region_style};
use crate::key_input::to_key;
use crate::minibuffer_view::{is_problem_list, render_minibuffer, render_scrim};
use crate::popup_view::{DocumentationScroll, DocumentationView, Placement, render_popup};
use crate::preview_view::render_preview;
use crate::table_view::{
    LIST_BOTTOM, ROW_PITCH, ROWS_TOP, TableCursor, fits_preview, render_hints, render_table,
};
use crate::widgets::{cursor_style, hsla, icon, modified_dot, pill, pill_separator};

/// Minified files can hold megabytes on one line; lay out only what can be on screen.
const MAX_RENDERED_LINE_BYTES: usize = 2048;
const TITLE_BAR_HEIGHT: f32 = 44.0;
const HEADER_FADE_HEIGHT: f32 = 80.0;
const FOOTER_FADE_HEIGHT: f32 = 56.0;
const TEXT_TOP: f32 = 64.0;
const ROW_MARGIN: f32 = 8.0;
const GUTTER_PADDING: f32 = 20.0;
const MIN_GUTTER_DIGITS: usize = 3;
/// Per mode; one popup shows a handful.
const MAX_HIGHLIGHTED_SNIPPETS: usize = 64;

pub struct EditorView {
    editor: Editor,
    focus_handle: FocusHandle,
    metrics: Option<Metrics>,
    scroll_remainder: Pixels,
    documentation_scroll: DocumentationScroll,
    /// How far the minibuffer input is scrolled sideways.
    input_scroll: ScrollHandle,
    /// Faces of code shown in popups, by mode and text.
    code_faces: HashMap<&'static str, HashMap<String, Highlights>>,
    /// The window buffer was last drawn as a table, so input maps to rows, not text.
    showing_table: bool,
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
            documentation_scroll: DocumentationScroll::default(),
            input_scroll: ScrollHandle::new(),
            code_faces: HashMap::new(),
            showing_table: false,
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

    pub fn choose_candidate(&mut self, index: usize, cx: &mut Context<Self>) {
        self.editor.choose_candidate(index);
        self.after_input(cx);
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
        let line_height = px(if self.showing_table {
            ROW_PITCH
        } else {
            font::CODE_LINE_HEIGHT
        });
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
        // An open prompt is modal: clicks choose candidates or do nothing. Table rows
        // handle their own clicks.
        let Some(metrics) = self
            .metrics
            .filter(|_| self.editor.minibuffer().is_none() && !self.showing_table)
        else {
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

    /// Bytes of `line` that can be on screen.
    fn visible_range(&self, line: usize) -> Range<usize> {
        let range = self
            .editor
            .window_buffer()
            .text()
            .line_range(line)
            .unwrap_or_default();
        range.start..range.end.min(range.start + MAX_RENDERED_LINE_BYTES)
    }

    fn display_line(&self, line: usize) -> DisplayLine {
        let text = self.editor.window_buffer().text();
        DisplayLine::new(&String::from_utf8_lossy(
            &text.bytes_in(self.visible_range(line)),
        ))
    }

    /// Each decorator's faces on `line`, mapped onto its display text.
    fn face_layers(&self, line: usize, display_line: &DisplayLine) -> Vec<Highlights> {
        let visible = self.visible_range(line);
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

    fn render_line(&self, line: usize, gutter_width: Pixels) -> AnyElement {
        let buffer = self.editor.window_buffer();
        let is_current = line == buffer.point_line();
        let mut display_line = self.display_line(line);
        let mut layers = self.face_layers(line, &display_line);
        if let Some(columns) = self.region_columns(line) {
            layers.push(vec![(display_line.cell_range(columns), region_style())]);
        }
        if is_current {
            let cursor = display_line.cursor_cell(buffer.point_column());
            layers.push(vec![(cursor, cursor_style())]);
        }
        let content = StyledText::new(display_line.text).with_highlights(combine(&layers));
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

    /// Popups next to the text they belong to, e.g. completions under the word or the
    /// documentation of a symbol.
    fn render_popups(
        &mut self,
        metrics: Metrics,
        gutter_width: Pixels,
        window: &Window,
    ) -> Vec<AnyElement> {
        let window_size = window.viewport_size();
        let line_height = px(font::CODE_LINE_HEIGHT);
        let popups = self.editor.popups();
        self.documentation_scroll.update(&popups);
        self.highlight_code_in(&popups);
        let code_faces = |code: &Code| self.faces_of(code);
        let view = DocumentationView {
            code_faces: &code_faces,
            scroll: &self.documentation_scroll.handle,
        };
        popups
            .iter()
            .filter_map(|popup| {
                let line = self.editor.window_buffer().text().line_of(popup.anchor);
                let row = line.checked_sub(self.editor.viewport.first_line)?;
                let line_top = px(TEXT_TOP) + line_height * row as f32;
                if line_top >= window_size.height {
                    return None;
                }
                let visible = self.visible_range(line);
                let offset = popup.anchor.clamp(visible.start, visible.end) - visible.start;
                let display_line = self.display_line(line);
                let byte = display_line.display_range(offset..offset).start;
                let column = display_line.text[..byte].chars().count();
                let anchor_left =
                    px(ROW_MARGIN) + gutter_width + metrics.char_width * column as f32;
                let placement = Placement {
                    anchor_left,
                    line_top,
                    line_bottom: line_top + line_height,
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

    /// The window buffer's lines from the viewport on, with the popups placed over them.
    fn render_text(&mut self, window: &Window) -> (AnyElement, Vec<AnyElement>) {
        let metrics = self.metrics(window);
        // Point stays above the footer fade; lines under it are drawn but not counted.
        let line_height = px(font::CODE_LINE_HEIGHT);
        let text_height = window.viewport_size().height - px(TEXT_TOP);
        let clear_rows = ((text_height - px(FOOTER_FADE_HEIGHT)) / line_height).floor();
        let drawn_rows = (text_height / line_height).ceil() as usize;
        self.editor
            .viewport
            .set_height(clear_rows.max(1.0) as usize);

        let gutter_width = self.gutter_width(metrics);
        let first_line = self.editor.viewport.first_line;
        let line_count = self.editor.window_buffer().text().line_count();
        let last_line = (first_line + drawn_rows).min(line_count);
        let popups = self.render_popups(metrics, gutter_width, window);
        let lines = (first_line..last_line).map(|line| self.render_line(line, gutter_width));
        let body = div()
            .absolute()
            .top(px(TEXT_TOP))
            .left_0()
            .right_0()
            .flex()
            .flex_col()
            .children(lines)
            .into_any_element();
        (body, popups)
    }

    fn run_command(&mut self, name: &str, cx: &mut Context<Self>) {
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

    /// A plugin's group of the status pill, e.g. problem counts; a click runs its command.
    /// One without a command is a plain label, like the mode.
    fn render_segment(segment: StatusSegment, cx: &mut Context<Self>) -> impl IntoElement {
        let text_color = match (segment.active, segment.command) {
            (true, _) => color::FG_MAIN,
            (false, Some(_)) => color::FG_KBD,
            (false, None) => color::FG_FAINT,
        };
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .h(px(24.0))
            .px(px(8.0))
            .rounded(px(6.0))
            .when(segment.active, |button| button.bg(rgb(color::BG_SELECTION)))
            .font_family(if segment.command.is_some() {
                font::MONO
            } else {
                font::UI
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
                let item_color = match item.tone {
                    Tone::Plain => text_color,
                    Tone::Accent => color::ACCENT,
                    Tone::Danger => color::DANGER,
                    Tone::Warning => color::WARNING,
                };
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
            // A table's selected row is its position.
            .when(!self.showing_table, |status| {
                status
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
        let table = self.editor.table(first_line..first_line + table_rows);
        self.showing_table = table.is_some();
        let (body, hints, popups) = match table {
            Some(mut table) => {
                self.editor.viewport.set_height(table_rows);
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
                let body = render_table(table, first_line, cursor, side, cx).into_any_element();
                (body, Some(hints), Vec::new())
            }
            None => {
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
            .child(body)
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(FOOTER_FADE_HEIGHT))
                    .bg(fade(0.0, 0.0)),
            )
            .children(hints)
            .children(status_below_scrim)
            .when(minibuffer.is_some(), |root| root.child(render_scrim()))
            .child(self.render_header())
            .children(popups)
            .children(status_above_scrim)
            .children(minibuffer)
    }
}

/// Black fading to transparent along `angle`, solid until `solid_until` (0..1).
fn fade(angle: f32, solid_until: f32) -> gpui::Background {
    linear_gradient(
        angle,
        linear_color_stop(hsla(color::BG_MAIN), solid_until),
        linear_color_stop(rgba(0x00000000), 1.0),
    )
}
