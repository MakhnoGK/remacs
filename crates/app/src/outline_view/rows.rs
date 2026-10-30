use std::mem;

use gpui::{AnyElement, Context, Div, FontWeight, div, prelude::*, px, rgb, rgba, svg};
use remacs_core::{DiffRow, Fact, FactValue, OutlineRow, RefChip, RefKind, Tone};
use remacs_theme::{color, font, glass};

use super::{
    DIFF_INDENT, DIFF_LINE_HEIGHT, FACT_HEIGHT, FACTS_TOP, HEADING_HEIGHT, ROW_HEIGHT, chip_frame,
    lift_selected,
};
use crate::assets::{CHEVRON_DOWN_ICON, CHEVRON_ICON};
use crate::editor_view::EditorView;
use crate::table_view::clickable;
use crate::widgets::{StyleExt, line_counts, shadow, state_letter, tone_color};

/// The facts over a rule, if there are any.
pub(super) fn render_facts(facts: Vec<Fact>, cx: &mut Context<EditorView>) -> Option<Div> {
    (!facts.is_empty()).then(|| {
        div()
            .flex()
            .flex_col()
            .flex_none()
            .pt(px(FACTS_TOP))
            .px(px(4.0))
            .pb(px(10.0))
            .mb(px(6.0))
            .border_b_1()
            .border_color(rgba(glass::ink(0x0f)))
            .children(facts.into_iter().map(|fact| render_fact(fact, cx)))
    })
}

fn render_fact(fact: Fact, cx: &mut Context<EditorView>) -> Div {
    let value = match fact.value {
        FactValue::Ref(chip) => ref_chip(chip),
        FactValue::Code(text) => div().flex_none().mono_status(color::fg_body()).child(text),
    };
    let (note_size, note_color) = if fact.note_is_title {
        (font::ui_size(), color::fg_body())
    } else {
        (font::status_size(), color::fg_dim())
    };
    let label = div().flex_none().w(px(64.0)).status_text(color::fg_faint());
    let note = div().fill_truncated().text(note_size, note_color);
    let buttons = fact.buttons.into_iter().map(|button| {
        let element = chip_frame(24.0, 10.0, 6.0)
            .bordered(glass::control_border())
            .bg(rgba(glass::control()))
            .text_size(px(font::status_size()))
            .shadow(vec![shadow(glass::shadow(0x80), 1.0, 2.0, 0.0)])
            .child(button.label);
        clickable(element, button.action, cx)
    });
    div()
        .row(10.0)
        .h(px(FACT_HEIGHT))
        .px(px(6.0))
        .whitespace_nowrap()
        .child(label.child(fact.label))
        .child(value)
        .child(note.child(fact.note))
        .children(buttons)
}

/// A branch or tag, colored by what it is.
pub fn ref_chip(chip: RefChip) -> Div {
    let tint = |color: u32, alpha: u32| color << 8 | alpha;
    let accent = |alpha: u32| tint(color::accent(), alpha);
    // A color of 0 is transparent.
    let (background, foreground, border) = match chip.kind {
        RefKind::Head => (accent(0x33), color::fg_ref_head(), accent(0x73)),
        RefKind::Main => (tint(color::keyword(), 0x29), color::fg_ref_main(), 0),
        RefKind::Branch => (tint(color::type_name(), 0x24), color::fg_ref_branch(), 0),
        RefKind::Remote => (0, color::fg_ref_remote(), glass::ink(0x29)),
        RefKind::Tag => (glass::chip(), color::fg_body(), 0),
    };
    chip_frame(20.0, 7.0, 5.0)
        .bordered(border)
        .bg(rgba(background))
        .font_family(font::mono())
        .text(font::small_size(), foreground)
        .child(chip.name)
}

/// The selection's text color on a selected row, `plain` on another.
fn lit(selected: bool, plain: u32) -> u32 {
    if selected { color::fg_alt() } else { plain }
}

fn render_heading(title: &str, count: usize, open: bool, tone: Tone, selected: bool) -> Div {
    let count_color = match tone {
        Tone::Faint => color::fg_line_number(),
        tone => tone_color(tone, color::fg_line_number()),
    };
    let chevron = if open { CHEVRON_DOWN_ICON } else { CHEVRON_ICON };
    let chevron = svg().path(chevron).flex_none().size(px(10.0)).text_color(rgb(color::fg_faint()));
    let title = title.to_uppercase();
    chip_frame(HEADING_HEIGHT, 10.0, 8.0)
        .gap(px(8.0))
        .when(selected, |heading| heading.bg(rgba(glass::selection())))
        .text(font::small_size(), lit(selected, color::fg_faint()))
        .child(chevron)
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
        .child(div().text_color(rgb(count_color)).child(count.to_string()))
}

fn row_frame(selected: bool) -> Div {
    lift_selected(chip_frame(ROW_HEIGHT, 10.0, 8.0), selected).gap(px(10.0)).whitespace_nowrap()
}

/// A run of diff lines in their frame, if there are any.
fn diff_frame(lines: Vec<Div>) -> Option<AnyElement> {
    (!lines.is_empty()).then(|| {
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
            .bg(rgba(glass::well()))
            .bordered(glass::ink(0x0f))
            .font_family(font::mono())
            .text_size(px(font::status_size()))
            .line_height(px(DIFF_LINE_HEIGHT))
            .children(lines)
            .into_any_element()
    })
}

/// `row` as drawn, without what a click on it does; a diff line belongs in a diff's frame.
fn row_body(row: OutlineRow, selected: bool) -> Div {
    match row {
        OutlineRow::Diff(diff) => diff_line(diff, selected),
        OutlineRow::Heading { title, count, open, tone } => {
            render_heading(&title, count, open, tone, selected)
        }
        OutlineRow::File { state, name, directory, lines, note, .. } => {
            let faint = lit(selected, color::fg_faint());
            let end = match note {
                Some((note, tone)) => {
                    let note_color = tone_color(tone, color::fg_dim());
                    Some(div().flex_none().status_text(note_color).child(note))
                }
                None => lines.map(|lines| line_counts(lines, selected)),
            };
            row_frame(selected)
                .child(state_letter(Some(state), selected).w(px(12.0)))
                .child(div().flex_none().font_family(font::mono()).child(name))
                .child(div().fill_truncated().status_text(faint).child(directory))
                .children(end)
        }
        OutlineRow::Item { name, description } => {
            let name_color = lit(selected, color::fg_dim());
            // Only the description takes the row's color; the name has its own.
            row_frame(selected)
                .text_color(rgb(color::fg_body()))
                .child(div().flex_none().mono_status(name_color).child(name))
                .child(div().fill_truncated().child(description))
        }
    }
}

/// `rows`, each with its index, as drawn: the `selected` one lit, each given its clicks by
/// `click`, told its index and whether it is a heading, and each run of diff lines in a frame.
pub(super) fn render_rows(
    rows: impl IntoIterator<Item = (usize, OutlineRow)>,
    selected: Option<usize>,
    mut click: impl FnMut(Div, usize, bool) -> Div,
) -> Vec<AnyElement> {
    let mut children = Vec::new();
    let mut diff_block = Vec::new();
    for (index, row) in rows {
        let is_diff = matches!(row, OutlineRow::Diff(_));
        let is_heading = matches!(row, OutlineRow::Heading { .. });
        let body = click(row_body(row, selected == Some(index)), index, is_heading);
        if is_diff {
            diff_block.push(body);
        } else {
            children.extend(diff_frame(mem::take(&mut diff_block)));
            children.push(body.into_any_element());
        }
    }
    children.extend(diff_frame(diff_block));
    children
}

fn diff_line(diff: DiffRow, selected: bool) -> Div {
    let (prefix, text, foreground, background) = match diff {
        DiffRow::Hunk(text) => ("", text, color::docstring(), Some(rgba(glass::ink(0x0a)))),
        DiffRow::Context(text) => ("", text, color::fg_kbd(), None),
        DiffRow::Added(text) => ("+", text, color::fg_added(), Some(rgba(glass::added()))),
        DiffRow::Removed(text) => ("-", text, color::fg_removed(), Some(rgba(glass::removed()))),
    };
    div()
        .flex()
        .flex_none()
        .h(px(DIFF_LINE_HEIGHT))
        .whitespace_nowrap()
        .overflow_hidden()
        .text_color(rgb(foreground))
        .when_some(background, |row, background| row.bg(background))
        .when(selected, |row| row.bg(rgba(glass::selection())))
        .child(div().flex_none().w(px(16.0)).text_center().child(prefix))
        .child(text)
}
