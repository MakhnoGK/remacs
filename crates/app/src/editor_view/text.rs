use std::collections::HashMap;
use std::ops::{Range, RangeInclusive};
use std::time::Instant;

use gpui::{
    AnyElement, Bounds, Context, Div, Pixels, Point, StyledText, Window, div, font, point,
    prelude::*, px, rgb, rgba, size, svg,
};
use remacs_core::{
    BreakpointIcon, Buffer, ChangedLines, Cursor, Face, GutterIcon, GutterMark, Icon, LineChange,
    LineExtra, ShownLines, Span, TextLabel,
};
use remacs_theme::font::CodeFont;
use remacs_theme::{color, font, glass};

use super::{
    EditorView, FOOTER_FADE_HEIGHT, MeasuredFonts, Metrics, ROW_MARGIN, ROW_RADIUS, TEXT_TOP,
    placed,
};
use crate::assets::{CHEVRON_DOWN_ICON, CHEVRON_ICON, EXECUTION_POINT_ICON, LOGPOINT_ICON};
use crate::display_line::{DisplayLine, display_width, slice_line};
use crate::highlights::{Highlights, combine, face_style, region_style, swatch_style};
use crate::settings::{CursorHeight, FoldToggles, SwatchesSettings};
use crate::widgets::{
    Edge, PressExt, SCROLL_FADE_SIZE, StyleExt, block_cursor, cursor_text_style, edge_fade,
    hollow_cursor, icon, over, tone_color,
};

/// Minified files can hold megabytes on one line; lay out only what can be on screen.
const MAX_RENDERED_LINE_BYTES: usize = 2048;
/// Columns point keeps from a side with text scrolled past it, clear of the side fades;
/// Zed keeps 5, Vim users often set `sidescrolloff` to 5–8.
const SIDE_MARGIN_COLUMNS: usize = 7;
const MIN_GUTTER_DIGITS: usize = 3;
/// The column of marks left of the line numbers, e.g. where a program can be run from.
const MARKS_WIDTH: f32 = 24.0;
// The gutter right of the line numbers, in their digit widths so that it grows with them
// as the text is zoomed: a gap, the fold toggles, the changed lines' marks, a gap.
const NUMBER_GAP: f32 = 0.2;
const FOLD_COLUMN: f32 = 1.4;
const CHANGE_COLUMN: f32 = 1.0;
const TEXT_GAP: f32 = 0.4;
const CHANGE_BAR_WIDTH: f32 = 0.4;
const CHANGE_TICK_HEIGHT: f32 = 0.4;
/// Pixels between a bar and the rows above and below it.
const CHANGE_BAR_GAP: f32 = 2.0;
/// Shows or hides a block, run by a click on its toggle or its count of hidden lines.
const HS_TOGGLE_HIDING: &str = "hs-toggle-hiding";
/// The gutter, over which the toggles of shown blocks appear.
const GUTTER_GROUP: &str = "gutter";

fn gutter_padding(digit_width: Pixels) -> Pixels {
    digit_width * (NUMBER_GAP + FOLD_COLUMN + CHANGE_COLUMN + TEXT_GAP)
}

impl EditorView {
    /// The font of the window buffer's text: the code font at the zoomed size.
    pub(super) fn text_font(&self) -> CodeFont {
        self.text_zoom.font(font::code())
    }

    /// Of the window buffer's text.
    pub(super) fn metrics(&mut self, window: &Window) -> Metrics {
        let text_font = self.text_font();
        cached_metrics(&mut self.metrics, text_font, window)
    }

    pub(super) fn gutter_width(&self, metrics: Metrics) -> Pixels {
        let digits = self
            .editor
            .window_buffer()
            .text()
            .line_count()
            .to_string()
            .len()
            .max(self.window_state.number_width.unwrap_or(0));
        // Some plugin may mark lines of the window buffer, so the gutter makes room for it.
        let marks = self
            .editor
            .gutter_marks(0..0)
            .map_or(px(0.0), |_| px(MARKS_WIDTH));
        marks
            + metrics.digit_width * digits.max(MIN_GUTTER_DIGITS) as f32
            + gutter_padding(metrics.digit_width)
    }

    /// Puts point on `line` and runs the command of its gutter mark.
    fn click_gutter_mark(&mut self, line: usize, command: &'static str, cx: &mut Context<Self>) {
        if self.editor.minibuffer().is_some() {
            return;
        }
        self.editor.set_point_at(line, 0);
        self.editor.run_command(command);
        self.after_input(cx);
    }

    /// The part of `line` in the drawn columns, and the bytes of the buffer it shows.
    pub(super) fn display_line(&self, line: usize) -> (DisplayLine, Range<usize>) {
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
        self.window_state.sideways.shown() as usize
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

    /// Each decorator's `decorations` of the `visible` bytes, mapped onto their display text.
    fn face_layers(
        visible: &Range<usize>,
        decorations: Vec<Vec<Span>>,
        display_line: &DisplayLine,
        swatches: SwatchesSettings,
    ) -> Vec<Highlights> {
        let to_line = |offset: usize| offset.clamp(visible.start, visible.end) - visible.start;
        decorations
            .into_iter()
            .map(|spans| {
                spans
                    .into_iter()
                    .filter_map(|span| {
                        let style = match span.face {
                            Face::Swatch(_) if !swatches.inline => return None,
                            Face::Swatch(color) => swatch_style(color, swatches.thickness),
                            face => face_style(face),
                        };
                        let range = to_line(span.range.start)..to_line(span.range.end);
                        Some((display_line.display_range(range), style))
                    })
                    .collect()
            })
            .collect()
    }

    /// Characters of `line` inside the active region; past the end means the newline too.
    pub(super) fn region_columns(&self, line: usize) -> Option<Range<usize>> {
        let buffer = self.editor.window_buffer();
        region_columns(buffer, buffer.region()?, line)
    }

    /// The row of `line`, shown as `display_line` with the decorators' faces on its
    /// `visible` bytes, its gutter mark in `mark_slot` when the gutter has a column for
    /// marks, the count of lines `folded` when it starts a hidden block, `labels` over its
    /// text and the part of the `rectangle` region on it, and whether the line goes on past
    /// its right edge.
    #[allow(clippy::too_many_arguments)]
    fn render_line(
        &self,
        line: usize,
        (mut display_line, visible): (DisplayLine, Range<usize>),
        decorations: Vec<Vec<Span>>,
        metrics: Metrics,
        gutter_width: Pixels,
        (mark_slot, folded): (Option<Div>, Option<Div>),
        extra: Option<LineExtra>,
        labels: Vec<TextLabel>,
        rectangle: Option<&(RangeInclusive<usize>, Range<usize>)>,
    ) -> (AnyElement, bool) {
        let LineExtra {
            band,
            fill,
            number,
            note,
            ..
        } = extra.unwrap_or_default();
        let fill_alpha = u32::from(self.diff.band_opacity) * 0xff / 100;
        let fill = fill.map(|tone| (tone_color(tone, color::fg_main()) << 8) | fill_alpha);
        let code_font = self.text_font();
        let row_height = px(code_font.line_height);
        let buffer = self.editor.window_buffer();
        // Another window shows only where its point is, as Emacs does: a hollow cursor, no
        // current line, no region, no other cursors.
        let selected = self.drawing_selected();
        let on_point = line == buffer.point_line();
        let is_current = on_point && selected;
        let mut layers = Self::face_layers(&visible, decorations, &display_line, self.swatches);
        let line_range = buffer.text().line_range(line).unwrap_or_default();
        let cursors = if selected { buffer.cursors() } else { &[] };
        let rectangle = rectangle
            .filter(|_| selected)
            .and_then(|rectangle| rectangle_columns(buffer, rectangle, line, &line_range));
        let regions: Vec<_> = buffer
            .region()
            .filter(|_| selected && !buffer.is_rectangle())
            .into_iter()
            .chain(cursors.iter().filter_map(Cursor::region))
            .filter_map(|region| region_columns(buffer, region, line))
            .chain(rectangle)
            .map(|columns| (display_line.cell_range(columns), region_style()))
            .collect();
        if !regions.is_empty() {
            layers.push(regions);
        }
        let on_line = cursors.partition_point(|cursor| cursor.point < line_range.start)
            ..cursors.partition_point(|cursor| cursor.point <= line_range.end);
        let points = cursors[on_line]
            .iter()
            .map(|cursor| cursor.point)
            .chain(on_point.then(|| buffer.point()));
        let height = match self.cursor_height {
            CursorHeight::Line => row_height,
            CursorHeight::Font => metrics.glyph_height.min(row_height),
        };
        let mut cursor_cells = Vec::new();
        // Point takes typed text here unless a prompt does.
        let takes_text = is_current && self.editor.minibuffer().is_none();
        let mut marked_at = None;
        let cursor: Vec<_> = points
            .map(|point| {
                let cell = display_line.cursor_cell(buffer.text().char_column(point));
                let column = display_line.text[..cell.start].chars().count();
                let left = metrics.char_width * column as f32;
                // Hollow too while the pane has the keys, as the terminal's is while it has not.
                let block = if selected && !self.editor.pane_focused() {
                    cursor_cells.push((cell.clone(), cursor_text_style()));
                    block_cursor()
                } else {
                    hollow_cursor()
                };
                let block = block
                    .absolute()
                    .left(left)
                    .top((row_height - height) / 2.0)
                    .w(metrics.char_width)
                    .h(height);
                if !(takes_text && point == buffer.point()) {
                    return block;
                }
                marked_at = Some((left, display_line.text[cell].to_owned()));
                block.child(self.composition.track_caret())
            })
            .collect();
        layers.push(cursor_cells);
        // Over the characters they stand on, as avy draws them; found from the bytes drawn,
        // as the faces are, not counted from the start of a line that may be megabytes.
        let label_elements: Vec<_> = labels
            .into_iter()
            // At the end too: a blank line's label is on its newline.
            .filter(|label| visible.start <= label.offset && label.offset <= visible.end)
            .map(|label| {
                let at = label.offset - visible.start;
                let shown = display_line.display_range(at..at + 1).start;
                let column = display_line.text[..shown].chars().count();
                div()
                    .absolute()
                    .top_0()
                    .left(metrics.char_width * column as f32)
                    .h(row_height)
                    .bg(rgb(color::accent()))
                    .text_color(rgb(color::bg_main()))
                    .child(label.text)
            })
            .collect();
        let marks_width = mark_slot.as_ref().map_or(px(0.0), |_| px(MARKS_WIDTH));
        let scrolled_past = self.window_state.sideways.shown() - display_line.first_column() as f32;
        let (hidden_start, continues) = (display_line.has_hidden_start(), display_line.continues);
        let content = StyledText::new(display_line.text).with_highlights(combine(&layers));
        // The text fades into what the row is drawn on.
        let (number_color, under) = match is_current {
            true => (
                color::fg_main(),
                over(glass::line_highlight(), glass::window_tint()),
            ),
            false => (color::fg_line_number(), rgba(glass::window_tint()).into()),
        };
        // Over the text after point, which it does not move, with the cursor after it.
        let marked = marked_at.and_then(|(left, covered)| {
            let text = self.composition.render_marked()?;
            let cursor = block_cursor().w(metrics.char_width).h(height);
            let marked = div().absolute().left(left).h(row_height).bg(under);
            Some(marked.row(0.0).child(text).child(cursor.child(covered)))
        });

        let text = div().flex().child(content).children(folded);
        let row = div()
            .flex()
            .flex_none()
            .h(row_height)
            .mx(px(ROW_MARGIN))
            // Filled rows run together, like a diff's added lines.
            .when(fill.is_none(), |row| row.rounded(px(ROW_RADIUS)))
            .when_some(fill, |row, fill| row.bg(rgba(fill)))
            .when(is_current, |row| {
                row.bg(match fill {
                    Some(fill) => over(glass::line_highlight(), fill),
                    None => rgba(glass::line_highlight()).into(),
                })
            })
            .when_some(band, |row, tone| {
                let band = tone_color(tone, color::fg_main());
                row.bg(rgba((band << 8) | 0x26))
                    .bordered((band << 8) | 0x4d)
            })
            .whitespace_nowrap()
            .line_height(row_height)
            .children(mark_slot.map(|slot| {
                let slot = slot.flex_none().w(px(MARKS_WIDTH)).h(row_height);
                slot.flex().items_center().justify_center()
            }))
            .child(
                div()
                    .flex_none()
                    .w(gutter_width - marks_width)
                    .pr(gutter_padding(metrics.digit_width))
                    .text_right()
                    .text(code_font.line_number_size(), number_color)
                    .child(number.unwrap_or_else(|| (line + 1).to_string())),
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
                            .child(text.children(note.map(|note| {
                                let note_text = div().status_text(color::fg_faint()).child(note);
                                note_text.ml(metrics.char_width * 4.0)
                            })))
                            .children(label_elements)
                            .children(marked),
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

    /// The gutter's mark slots for `lines`, mapped by line, when some plugin may mark the
    /// window buffer; a click on a mark runs its command.
    fn render_mark_slots(
        &self,
        lines: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Option<HashMap<usize, Div>> {
        let marks = self.editor.gutter_marks(lines.clone())?;
        let mut by_line: HashMap<usize, Vec<GutterMark>> = HashMap::new();
        for mark in marks {
            by_line.entry(mark.line).or_default().push(mark);
        }
        let empty_click = self.editor.gutter_click();
        let slots = lines
            .map(|line| {
                let mut marks = by_line.remove(&line).unwrap_or_default();
                marks.sort_by_key(|mark| mark.icon);
                // A run mark gives way to a breakpoint; the execution point goes over both.
                let execution = marks
                    .iter()
                    .position(|mark| mark.icon == GutterIcon::ExecutionPoint)
                    .map(|index| marks.remove(index));
                let shown = marks.pop();
                let command = shown
                    .as_ref()
                    .map(|mark| mark.command)
                    .or(empty_click)
                    .or(execution.as_ref().map(|mark| mark.command));
                let execution = execution.map(|_| {
                    let arrow = div().absolute().left(px(4.0)).top(px(5.0));
                    arrow.child(gutter_glyph(GutterIcon::ExecutionPoint))
                });
                let slot = div()
                    .relative()
                    .children(shown.map(|mark| gutter_glyph(mark.icon)))
                    .children(execution)
                    .when_some(command, |slot, command| {
                        slot.cursor_pointer().on_press(cx, move |view, cx| {
                            view.click_gutter_mark(line, command, cx)
                        })
                    });
                (line, slot)
            })
            .collect();
        Some(slots)
    }

    /// The controls of the blocks that start on the `drawn` lines, over a gutter that
    /// ends at `text_left`; a click on one shows or hides its block, as `hs-toggle-hiding`
    /// on its line does. The toggles go over the gutter; the counts of lines hidden blocks
    /// hide go after the text of the line each starts on.
    fn render_fold_controls(
        &self,
        drawn: &[usize],
        shown_lines: &ShownLines,
        (metrics, text_left): (Metrics, Pixels),
        cx: &mut Context<Self>,
    ) -> (AnyElement, HashMap<usize, Div>) {
        let row_height = px(self.text_font().line_height);
        let gutter_size = size(text_left, row_height * drawn.len() as f32);
        let gutter = placed(Bounds::new(Point::default(), gutter_size)).group(GUTTER_GROUP);
        let buffer = self.editor.window_buffer();
        let starts: Vec<usize> = (drawn.first().zip(drawn.last()))
            .and_then(|(&first, &last)| self.editor.blocks(buffer, first..last + 1))
            .unwrap_or_default()
            .into_iter()
            .filter(|block| !block.hidden.is_empty())
            .map(|block| block.lines.start)
            .collect();
        let point_line = self.drawing_selected().then(|| buffer.point_line());
        let digit_width = metrics.digit_width;
        let mut toggles = Vec::new();
        let mut counts = HashMap::new();
        for (row, &line) in drawn.iter().enumerate() {
            let hidden = shown_lines.hidden_after(line);
            if hidden.is_none() && !starts.contains(&line) {
                continue;
            }
            let clickable = |element: Div, cx: &mut Context<Self>| {
                (element.cursor_pointer()).on_press(cx, move |view, cx| {
                    view.click_gutter_mark(line, HS_TOGGLE_HIDING, cx)
                })
            };
            let always = hidden.is_some()
                || self.folds.toggles == FoldToggles::Always
                || point_line == Some(line);
            let chevron = if hidden.is_some() {
                CHEVRON_ICON
            } else {
                CHEVRON_DOWN_ICON
            };
            let origin = point(
                text_left - digit_width * (TEXT_GAP + CHANGE_COLUMN + FOLD_COLUMN),
                row_height * row as f32,
            );
            let toggle = placed(Bounds::new(
                origin,
                size(digit_width * FOLD_COLUMN, row_height),
            ))
            .flex()
            .items_center()
            .justify_center()
            .child(
                svg()
                    .path(chevron)
                    .size(digit_width * FOLD_COLUMN)
                    .text_color(rgb(color::fg_line_number())),
            )
            .when(!always, |toggle| {
                toggle
                    .opacity(0.0)
                    .group_hover(GUTTER_GROUP, |style| style.opacity(1.0))
            });
            toggles.push(clickable(toggle, cx));
            if let Some(hidden) = hidden {
                let folded = div()
                    .ml(metrics.char_width * 2.0)
                    .px(metrics.char_width * 0.5)
                    .rounded(px(ROW_RADIUS))
                    .bg(rgba(glass::line_highlight()))
                    .text_color(rgb(color::fg_faint()))
                    .child(format!("⋯ {hidden} lines"));
                counts.insert(line, clickable(folded, cx));
            }
        }
        (gutter.children(toggles).into_any_element(), counts)
    }

    /// The window buffer's lines from the viewport on, with the popups placed over them.
    pub(super) fn render_text(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, Vec<AnyElement>) {
        let metrics = self.metrics(window);
        let (frame, reserved) = (self.window_state.frame.size, self.window_state.reserved);
        // Point stays above the footer fade; lines under it are drawn but not counted.
        let line_height = px(self.text_font().line_height);
        let text_height = frame.height - px(TEXT_TOP) - reserved;
        let clear_rows = ((text_height - px(FOOTER_FADE_HEIGHT)) / line_height)
            .floor()
            .max(1.0) as usize;
        let drawn_rows = (text_height / line_height).ceil() as usize;
        self.editor.viewport.set_height(clear_rows);
        // Only the top fade is over lines point can be on.
        self.editor.viewport.margins = (1, 0);
        let gutter_width = self.gutter_width(metrics);
        let text_width = frame.width
            - self.window_state.text_left
            - px(2.0 * ROW_MARGIN + ROW_RADIUS)
            - gutter_width;
        self.editor.viewport.width = (text_width / metrics.char_width).floor().max(1.0) as usize;
        self.editor.viewport.column_margin = SIDE_MARGIN_COLUMNS;
        if !self.window_state.scrolled_sideways {
            let point_column = self.point_display_column();
            self.editor.viewport.ensure_column_visible(point_column);
        }

        let top = self.advance_scroll(clear_rows, window);
        let shown_lines = self.editor.window_buffer().shown_lines();
        let first_row = top as usize;
        // One more for the line partly scrolled in at the bottom.
        let drawn: Vec<usize> = (first_row..(first_row + drawn_rows + 1).min(shown_lines.rows()))
            .map(|row| shown_lines.line_at(row))
            .collect();
        // With the lines folds hide between them.
        let first_line = drawn.first().copied().unwrap_or(0);
        let last_line = drawn.last().map_or(0, |last| last + 1);
        // Popups belong to the selected window.
        let popups = if self.drawing_selected() {
            self.render_popups(metrics, gutter_width, window.viewport_size())
        } else {
            Vec::new()
        };
        let mut mark_slots = self.render_mark_slots(first_line..last_line, cx);
        let text_left = px(ROW_MARGIN) + gutter_width;
        let (toggles, mut folded) =
            self.render_fold_controls(&drawn, &shown_lines, (metrics, text_left), cx);
        let mut extras: HashMap<usize, LineExtra> = self
            .editor
            .line_extras(first_line..last_line)
            .into_iter()
            .map(|extra| (extra.line, extra))
            .collect();
        let mut labels: HashMap<usize, Vec<TextLabel>> = HashMap::new();
        let text = self.editor.window_buffer().text();
        for label in self.editor.text_labels(first_line..last_line) {
            labels
                .entry(text.line_of(label.offset))
                .or_default()
                .push(label);
        }
        let shown: Vec<_> = drawn.iter().map(|&line| self.display_line(line)).collect();
        let visible: Vec<_> = shown.iter().map(|(_, visible)| visible.clone()).collect();
        let decorations = self.editor.decorations_by_line(&visible);
        // Once a frame: finding the columns reads the lines of point and mark.
        let buffer = self.editor.window_buffer();
        let rectangle = buffer.is_rectangle().then(|| buffer.rectangle()).flatten();
        let (lines, continued): (Vec<_>, Vec<_>) = (drawn.iter().copied())
            .zip(shown)
            .zip(decorations)
            .map(|((line, shown), decorations)| {
                let slot = mark_slots.as_mut().and_then(|slots| slots.remove(&line));
                let extra = extras.remove(&line);
                let labels = labels.remove(&line).unwrap_or_default();
                self.render_line(
                    line,
                    shown,
                    decorations,
                    metrics,
                    gutter_width,
                    (slot, folded.remove(&line)),
                    extra,
                    labels,
                    rectangle.as_ref(),
                )
            })
            .unzip();
        self.window_state.more_on_right = continued.contains(&true);
        let changes = self
            .editor
            .line_changes(first_line..last_line)
            .into_iter()
            .filter_map(|changed| {
                let shown = (first_line..last_line, &shown_lines, first_row);
                render_line_change(
                    changed,
                    shown,
                    (text_left, metrics.digit_width),
                    line_height,
                )
            });
        let glass = rgba(glass::window_tint()).into();
        let lines = div()
            .relative()
            .top(-line_height * top.fract())
            .flex()
            .flex_col()
            .children(lines)
            .children(changes)
            .child(toggles);
        let body = div()
            .absolute()
            .top(px(TEXT_TOP))
            .left(self.window_state.text_left)
            .right_0()
            .bottom(reserved)
            .overflow_hidden()
            .child(lines)
            .when(top > 0.0, |body| {
                // Over the top margin's line.
                body.child(edge_fade(Edge::Top, line_height.into(), glass))
            })
            .when(first_row + clear_rows < shown_lines.rows(), |body| {
                body.child(edge_fade(Edge::Bottom, FOOTER_FADE_HEIGHT, glass))
            })
            .into_any_element();
        (body, popups)
    }

    /// Moves where the content is drawn toward the viewport, `screen_rows` tall, and
    /// returns where it is drawn from; another buffer is shown at once. Asks for frames
    /// while it glides.
    pub(super) fn advance_scroll(&mut self, screen_rows: usize, window: &Window) -> f32 {
        let viewport = self.editor.viewport;
        let buffer = self.editor.window_buffer();
        let shown = buffer.shown_lines();
        let (name, first_row) = (buffer.name(), shown.row_of(viewport.first_line));
        let hidden = buffer.text().line_count() - shown.rows();
        if self.window_state.scrolled_hidden != hidden {
            self.window_state.scrolled_hidden = hidden;
            self.window_state.vertical.jump(first_row);
        }
        if self.window_state.scrolled_buffer != name {
            self.window_state.scrolled_buffer = name.to_owned();
            self.window_state.vertical.jump(first_row);
            self.window_state.sideways.jump(viewport.first_column);
            self.window_state.history.place(0.0);
            self.window_state.history_target = 0.0;
        }
        let now = Instant::now();
        let duration = self.scroll.animation_duration();
        let top = self
            .window_state
            .vertical
            .advance(first_row, now, duration, screen_rows as f32);
        self.window_state.sideways.advance(
            viewport.first_column,
            now,
            duration,
            viewport.width as f32,
        );
        if self.window_state.vertical.is_animating() || self.window_state.sideways.is_animating() {
            window.request_animation_frame();
        }
        top
    }
}

/// The metrics of `code_font` and the UI font, measured again only once either changes.
pub(super) fn cached_metrics(
    cache: &mut Option<(MeasuredFonts, Metrics)>,
    code_font: CodeFont,
    window: &Window,
) -> Metrics {
    let fonts = (code_font, font::ui(), font::ui_size());
    if let Some((measured, metrics)) = cache
        && *measured == fonts
    {
        return *metrics;
    }
    let code_font = &fonts.0;
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
        glyph_height: text_system.ascent(font_id, size) + text_system.descent(font_id, size).abs(),
        digit_width: advance(code_font.line_number_size(), '0'),
        mono_width: advance(font::mono_size(), 'm'),
        small_mono_width: advance(font::status_size(), 'm'),
        // 'n' is about as wide as a letter of text on average, spaces counted.
        ui_char_width: text_system
            .advance(
                text_system.resolve_font(&font(fonts.1.clone())),
                px(fonts.2),
                'n',
            )
            .map_or(px(fonts.2 * 0.55), |advance| advance.width),
    };
    *cache = Some((fonts, metrics));
    metrics
}

/// Characters of `line` inside `region`; past the end means the newline too.
fn region_columns(buffer: &Buffer, region: Range<usize>, line: usize) -> Option<Range<usize>> {
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

/// Characters of `line`, at `line_range`, inside `rectangle`'s lines and columns, none
/// where the line is too short to reach it.
fn rectangle_columns(
    buffer: &Buffer,
    (lines, columns): &(RangeInclusive<usize>, Range<usize>),
    line: usize,
    line_range: &Range<usize>,
) -> Option<Range<usize>> {
    if !lines.contains(&line) {
        return None;
    }
    let line_len = buffer.text().char_column(line_range.end);
    let columns = columns.start.min(line_len)..columns.end.min(line_len);
    (!columns.is_empty()).then_some(columns)
}

/// A gutter mark as drawn: run triangles and execution arrows are icons, breakpoints dots.
fn gutter_glyph(mark: GutterIcon) -> AnyElement {
    let dot = || div().flex_none().size(px(12.0)).rounded_full();
    match mark {
        GutterIcon::Run => icon(Icon::Run, color::vc_added()).into_any_element(),
        GutterIcon::ExecutionPoint => svg()
            .path(EXECUTION_POINT_ICON)
            .w(px(15.0))
            .h(px(12.0))
            .text_color(rgb(color::warning()))
            .into_any_element(),
        GutterIcon::Breakpoint(BreakpointIcon::Plain) => {
            dot().bg(rgb(color::error())).into_any_element()
        }
        GutterIcon::Breakpoint(BreakpointIcon::Conditional) => dot()
            .bg(rgb(color::error()))
            .flex()
            .items_center()
            .justify_center()
            .font_family(font::mono())
            .text(9.0, color::fg_main())
            .font_weight(gpui::FontWeight::BOLD)
            .child("?")
            .into_any_element(),
        GutterIcon::Breakpoint(BreakpointIcon::Log) => svg()
            .path(LOGPOINT_ICON)
            .size(px(12.0))
            .text_color(rgb(color::error()))
            .into_any_element(),
        GutterIcon::Breakpoint(BreakpointIcon::Disabled) => dot()
            .border(px(1.5))
            .border_color(rgb(color::fg_dim()))
            .into_any_element(),
        GutterIcon::Breakpoint(BreakpointIcon::Unverified) => dot()
            .border(px(1.5))
            .border_dashed()
            .border_color(rgb(color::error()))
            .opacity(0.85)
            .into_any_element(),
    }
}

/// A bar in the gutter beside added or modified lines, or a tick between the lines where
/// some were removed; `shown` are the drawn lines, which folds may hide some of, and the
/// row of the first, `text_left` where their text starts and `digit_width` that of the
/// line numbers.
fn render_line_change(
    changed: ChangedLines,
    (shown, shown_lines, first_row): (Range<usize>, &ShownLines, usize),
    (text_left, digit_width): (Pixels, Pixels),
    line_height: Pixels,
) -> Option<AnyElement> {
    let row = |line: usize| shown_lines.row_of(line) - first_row;
    let top = |line: usize| line_height * row(line) as f32;
    let column_left = text_left - digit_width * (TEXT_GAP + CHANGE_COLUMN);
    let mark = if changed.lines.is_empty() {
        div()
            .top(top(changed.lines.start) - digit_width * (CHANGE_TICK_HEIGHT / 2.0))
            .left(column_left)
            .w(digit_width * CHANGE_COLUMN)
            .h(digit_width * CHANGE_TICK_HEIGHT)
    } else {
        let lines = changed.lines.start.max(shown.start)..changed.lines.end.min(shown.end);
        if lines.is_empty() {
            return None;
        }
        let rows = row(lines.end - 1) + 1 - row(lines.start);
        div()
            .top(top(lines.start) + px(CHANGE_BAR_GAP))
            .left(column_left + digit_width * ((CHANGE_COLUMN - CHANGE_BAR_WIDTH) / 2.0))
            .w(digit_width * CHANGE_BAR_WIDTH)
            .h(line_height * rows as f32 - px(2.0 * CHANGE_BAR_GAP))
    };
    let color = match changed.change {
        LineChange::Added => glass::vc_added_mark(),
        LineChange::Modified => glass::vc_modified_mark(),
        LineChange::Removed => glass::vc_removed_mark(),
    };
    Some(
        mark.absolute()
            .rounded(px(2.0))
            .bg(rgba(color))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use remacs_text::Text;

    use super::*;

    fn buffer(text: &str) -> Buffer {
        Buffer::new("test", Text::from(text), "Fundamental")
    }

    #[test]
    fn region_columns_count_characters_and_the_newline_past_the_end() {
        let buffer = buffer("ab\nжcd\n");
        assert_eq!(region_columns(&buffer, 1..5, 0), Some(1..3));
        assert_eq!(region_columns(&buffer, 1..5, 1), Some(0..1));
        assert_eq!(region_columns(&buffer, 0..3, 1), None);
        assert_eq!(region_columns(&buffer, 8..9, 0), None);
    }

    #[test]
    fn rectangle_columns_stop_at_short_lines() {
        let buffer = buffer("abcdef\nab\n");
        let rectangle = (0..=1, 1..4);
        assert_eq!(
            rectangle_columns(&buffer, &rectangle, 0, &(0..6)),
            Some(1..4)
        );
        assert_eq!(
            rectangle_columns(&buffer, &rectangle, 1, &(7..9)),
            Some(1..2)
        );
        assert_eq!(rectangle_columns(&buffer, &rectangle, 2, &(10..10)), None);
        assert_eq!(rectangle_columns(&buffer, &(0..=1, 3..5), 1, &(7..9)), None);
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn face_layers_map_spans_onto_the_visible_bytes_and_drop_swatches_unless_inline() {
        let line = DisplayLine::new("cdef");
        let decorations = || {
            vec![vec![
                Span {
                    range: 0..3,
                    face: Face::Keyword,
                },
                Span {
                    range: 4..9,
                    face: Face::Swatch(0xff0000),
                },
            ]]
        };
        let ranges = |inline| -> Vec<Vec<Range<usize>>> {
            let swatches = SwatchesSettings {
                inline,
                ..Default::default()
            };
            EditorView::face_layers(&(2..6), decorations(), &line, swatches)
                .into_iter()
                .map(|layer| layer.into_iter().map(|(range, _)| range).collect())
                .collect()
        };
        assert_eq!(ranges(false), [vec![0..1]]);
        assert_eq!(ranges(true), [vec![0..1, 2..4]]);
    }
}
