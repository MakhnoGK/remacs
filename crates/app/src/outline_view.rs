use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Pixels, div, prelude::*, px, rgb, rgba, svg,
};
use remacs_core::{
    DiffRow, Fact, FactValue, LineCounts, Outline, OutlineRow, RefChip, RefKind, Tone,
};
use remacs_theme::{color, font};

use crate::assets::{CHEVRON_DOWN_ICON, CHEVRON_ICON};
use crate::editor_view::EditorView;
use crate::table_view::{LIST_BOTTOM, clickable};
use crate::widgets::{shadow, state_letter};

const PANE_TOP: f32 = 56.0;
const PANE_SIDE: f32 = 16.0;
const PANE_WIDTH: f32 = 480.0;
const PANE_PADDING: f32 = 8.0;
const ROW_GAP: f32 = 2.0;
const FACT_HEIGHT: f32 = 32.0;
/// Under the facts: their padding, the rule and the space after it.
const FACTS_BOTTOM: f32 = 10.0 + 1.0 + 6.0;
const FACTS_TOP: f32 = 4.0;
const HEADING_HEIGHT: f32 = 30.0;
const ROW_HEIGHT: f32 = 32.0;
const DIFF_LINE_HEIGHT: f32 = 20.0;
/// Around a diff: the space above and below it and its padding.
const DIFF_TOP: f32 = 2.0 + 4.0;
const DIFF_BOTTOM: f32 = 4.0 + 6.0;
const DIFF_INDENT: f32 = 30.0;

/// The most lines that can fit in a window this tall, before the rows are known.
pub fn max_outline_lines(window_height: Pixels) -> usize {
    (rows_height(window_height, 0) / DIFF_LINE_HEIGHT).ceil() as usize + 1
}

/// How many of `rows`, from the first, fit under `fact_count` facts in a window this tall.
pub fn fitting_rows(rows: &[OutlineRow], fact_count: usize, window_height: Pixels) -> usize {
    let room = rows_height(window_height, fact_count);
    let mut used = 0.0;
    for (index, row) in rows.iter().enumerate() {
        let is_diff = |row: Option<&OutlineRow>| matches!(row, Some(OutlineRow::Diff(_)));
        let mut height = match row {
            OutlineRow::Heading { .. } => HEADING_HEIGHT + ROW_GAP,
            OutlineRow::File { .. } | OutlineRow::Item { .. } => ROW_HEIGHT + ROW_GAP,
            OutlineRow::Diff(_) => DIFF_LINE_HEIGHT,
        };
        if is_diff(Some(row)) && !is_diff(index.checked_sub(1).and_then(|before| rows.get(before)))
        {
            height += DIFF_TOP + ROW_GAP;
        }
        if is_diff(Some(row)) && !is_diff(rows.get(index + 1)) {
            height += DIFF_BOTTOM;
        }
        used += height;
        if used > room {
            return index.max(1);
        }
    }
    rows.len()
}

fn rows_height(window_height: Pixels, fact_count: usize) -> f32 {
    let facts = if fact_count == 0 {
        0.0
    } else {
        FACTS_TOP + FACT_HEIGHT * fact_count as f32 + FACTS_BOTTOM
    };
    (window_height / px(1.0)) - PANE_TOP - LIST_BOTTOM - 2.0 * PANE_PADDING - facts
}

/// The outline's facts and its rows from `first_line` on, point's line selected, in a
/// pane at the left of the window.
pub fn render_outline(
    outline: Outline,
    first_line: usize,
    point_line: usize,
    cx: &mut Context<EditorView>,
) -> impl IntoElement {
    let Outline {
        facts,
        rows,
        toggle,
        activate,
        ..
    } = outline;
    let mut children: Vec<AnyElement> = Vec::new();
    if !facts.is_empty() {
        children.push(render_facts(facts, cx).into_any_element());
    }
    let mut diff_block: Vec<AnyElement> = Vec::new();
    let row_count = rows.len();
    for (index, row) in rows.into_iter().enumerate() {
        let line = first_line + index;
        let selected = line == point_line;
        let element = match row {
            OutlineRow::Diff(diff) => {
                diff_block.push(render_diff_line(diff, selected, line, activate, cx));
                None
            }
            OutlineRow::Heading {
                title,
                count,
                open,
                tone,
            } => Some(
                render_heading(&title, count, open, tone, selected).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        view.click_heading(line, toggle, cx);
                    }),
                ),
            ),
            OutlineRow::File {
                state,
                name,
                directory,
                lines,
                ..
            } => Some(clicks(
                row_frame(selected)
                    .child(state_letter(Some(state), selected).w(px(12.0)))
                    .child(div().flex_none().font_family(font::MONO).child(name))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(font::STATUS_SIZE))
                            .text_color(rgb(if selected {
                                color::FG_ALT
                            } else {
                                color::FG_FAINT
                            }))
                            .child(directory),
                    )
                    .children(lines.map(|lines| line_counts(lines, selected))),
                line,
                activate,
                cx,
            )),
            OutlineRow::Item { name, description } => Some(clicks(
                row_frame(selected)
                    .child(
                        div()
                            .flex_none()
                            .font_family(font::MONO)
                            .text_size(px(font::STATUS_SIZE))
                            .text_color(rgb(if selected {
                                color::FG_ALT
                            } else {
                                color::FG_DIM
                            }))
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(rgb(color::FG_BODY))
                            .child(description),
                    ),
                line,
                activate,
                cx,
            )),
        };
        if !diff_block.is_empty() && (element.is_some() || index + 1 == row_count) {
            children.push(diff_frame(std::mem::take(&mut diff_block)).into_any_element());
        }
        children.extend(element.map(IntoElement::into_any_element));
    }
    let pane = div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(PANE_WIDTH))
        .p(px(PANE_PADDING))
        .gap(px(ROW_GAP))
        .overflow_hidden()
        .bg(rgb(color::BG_PANE))
        .border_1()
        .border_color(rgba(0xffffff12))
        .rounded(px(12.0))
        .font_family(font::UI)
        .text_size(px(font::UI_SIZE))
        .children(children);
    div()
        .absolute()
        .top(px(PANE_TOP))
        .left(px(PANE_SIDE))
        .right(px(PANE_SIDE))
        .bottom(px(LIST_BOTTOM))
        .flex()
        .child(pane)
}

fn render_facts(facts: Vec<Fact>, cx: &mut Context<EditorView>) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pt(px(FACTS_TOP))
        .px(px(4.0))
        .pb(px(10.0))
        .mb(px(6.0))
        .border_b_1()
        .border_color(rgba(0xffffff0f))
        .children(facts.into_iter().map(|fact| {
            let value = match fact.value {
                FactValue::Ref(chip) => ref_chip(chip),
                FactValue::Code(text) => div()
                    .flex_none()
                    .font_family(font::MONO)
                    .text_size(px(font::STATUS_SIZE))
                    .text_color(rgb(color::FG_BODY))
                    .child(text),
            };
            let (note_size, note_color) = if fact.note_is_title {
                (font::UI_SIZE, color::FG_BODY)
            } else {
                (font::STATUS_SIZE, color::FG_DIM)
            };
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .h(px(FACT_HEIGHT))
                .px(px(6.0))
                .whitespace_nowrap()
                .child(
                    div()
                        .flex_none()
                        .w(px(64.0))
                        .text_size(px(font::STATUS_SIZE))
                        .text_color(rgb(color::FG_FAINT))
                        .child(fact.label),
                )
                .child(value)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(note_size))
                        .text_color(rgb(note_color))
                        .child(fact.note),
                )
                .children(fact.buttons.into_iter().map(|button| {
                    clickable(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .h(px(24.0))
                            .px(px(10.0))
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(rgba(0xffffff14))
                            .bg(rgb(color::BG_BUTTON))
                            .text_size(px(font::STATUS_SIZE))
                            .shadow(vec![shadow(0x00000080, 1.0, 2.0, 0.0)])
                            .child(button.label),
                        button.action,
                        cx,
                    )
                }))
        }))
}

/// A branch or tag, colored by what it is.
pub fn ref_chip(chip: RefChip) -> Div {
    let (background, foreground, border) = match chip.kind {
        RefKind::Head => (rgba(0x2fafff33), color::FG_REF_HEAD, rgba(0x2fafff73)),
        RefKind::Main => (rgba(0xb6a0ff29), color::FG_REF_MAIN, rgba(0x00000000)),
        RefKind::Branch => (rgba(0x6ae4b924), color::FG_REF_BRANCH, rgba(0x00000000)),
        RefKind::Remote => (rgba(0x00000000), color::FG_REF_REMOTE, rgba(0xffffff29)),
        RefKind::Tag => (rgb(color::BG_CHIP), color::FG_BODY, rgba(0x00000000)),
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(20.0))
        .px(px(7.0))
        .rounded(px(5.0))
        .border_1()
        .border_color(border)
        .bg(background)
        .font_family(font::MONO)
        .text_size(px(font::SMALL_SIZE))
        .text_color(rgb(foreground))
        .child(chip.name)
}

fn render_heading(title: &str, count: usize, open: bool, tone: Tone, selected: bool) -> Div {
    let count_color = match tone {
        Tone::Danger => color::DANGER,
        Tone::Accent => color::ACCENT,
        Tone::Warning => color::WARNING,
        Tone::Plain | Tone::Faint => color::FG_LINE_NUMBER,
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .h(px(HEADING_HEIGHT))
        .px(px(10.0))
        .rounded(px(8.0))
        .when(selected, |heading| heading.bg(rgb(color::BG_SELECTION)))
        .text_size(px(font::SMALL_SIZE))
        .text_color(rgb(if selected {
            color::FG_ALT
        } else {
            color::FG_FAINT
        }))
        .child(
            svg()
                .path(if open {
                    CHEVRON_DOWN_ICON
                } else {
                    CHEVRON_ICON
                })
                .flex_none()
                .size(px(10.0))
                .text_color(rgb(color::FG_FAINT)),
        )
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_uppercase()),
        )
        .child(div().text_color(rgb(count_color)).child(count.to_string()))
}

fn row_frame(selected: bool) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.0))
        .h(px(ROW_HEIGHT))
        .px(px(10.0))
        .rounded(px(8.0))
        .whitespace_nowrap()
        .when(selected, |row| {
            row.bg(rgb(color::BG_SELECTION))
                .shadow(vec![shadow(0x00000073, 1.0, 3.0, 0.0)])
        })
}

/// `+24 −3`, the zero side left out.
fn line_counts(lines: LineCounts, selected: bool) -> Div {
    let (added, removed) = if selected {
        (color::VC_ADDED_ON_SELECTION, color::VC_REMOVED_ON_SELECTION)
    } else {
        (color::VC_ADDED, color::VC_REMOVED)
    };
    let count = |text: String, text_color| div().text_color(rgb(text_color)).child(text);
    div()
        .flex()
        .flex_none()
        .gap(px(10.0))
        .font_family(font::MONO)
        .text_size(px(font::STATUS_SIZE))
        .when(lines.added > 0, |counts| {
            counts.child(count(format!("+{}", lines.added), added))
        })
        .when(lines.removed > 0, |counts| {
            counts.child(count(format!("−{}", lines.removed), removed))
        })
}

fn clicks(row: Div, line: usize, activate: &'static str, cx: &mut Context<EditorView>) -> Div {
    row.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |view, event: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            view.click_row(line, event.click_count, activate, cx);
        }),
    )
}

fn diff_frame(lines: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .ml(px(DIFF_INDENT))
        .mr(px(4.0))
        .mt(px(2.0))
        .mb(px(6.0))
        .py(px(4.0))
        .rounded(px(8.0))
        .overflow_hidden()
        .bg(rgb(color::BG_MAIN))
        .border_1()
        .border_color(rgba(0xffffff0f))
        .font_family(font::MONO)
        .text_size(px(font::STATUS_SIZE))
        .line_height(px(DIFF_LINE_HEIGHT))
        .children(lines)
}

fn render_diff_line(
    diff: DiffRow,
    selected: bool,
    line: usize,
    activate: &'static str,
    cx: &mut Context<EditorView>,
) -> AnyElement {
    let (prefix, text, foreground, background) = match diff {
        DiffRow::Hunk(text) => ("", text, color::DOCSTRING, Some(rgba(0xffffff0a))),
        DiffRow::Context(text) => ("", text, color::FG_KBD, None),
        DiffRow::Added(text) => ("+", text, color::FG_ADDED, Some(rgb(color::BG_ADDED))),
        DiffRow::Removed(text) => ("-", text, color::FG_REMOVED, Some(rgb(color::BG_REMOVED))),
    };
    let row = div()
        .flex()
        .flex_none()
        .h(px(DIFF_LINE_HEIGHT))
        .whitespace_nowrap()
        .overflow_hidden()
        .text_color(rgb(foreground))
        .when_some(background, |row, background| row.bg(background))
        .when(selected, |row| row.bg(rgb(color::BG_SELECTION)))
        .child(div().flex_none().w(px(16.0)).text_center().child(prefix))
        .child(text);
    clicks(row, line, activate, cx).into_any_element()
}
