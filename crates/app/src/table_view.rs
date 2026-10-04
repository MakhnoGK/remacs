use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    div, prelude::*, px, rgb, rgba, svg,
};
use remacs_core::{Action, Button, CellStyle, Column, Row, RowFace, Table};
use remacs_theme::{color, font};

use crate::assets::CHEVRON_ICON;
use crate::editor_view::EditorView;
use crate::widgets::{icon, kbd, pill, shadow};

const LIST_TOP: f32 = 52.0;
const LIST_SIDE: f32 = 16.0;
pub const LIST_BOTTOM: f32 = 60.0;
const BAR_HEIGHT: f32 = 32.0;
const HEADER_HEIGHT: f32 = 28.0;
const SECTION_GAP: f32 = 8.0;
pub const ROWS_TOP: f32 = LIST_TOP + BAR_HEIGHT + SECTION_GAP + HEADER_HEIGHT + SECTION_GAP;
const ROW_HEIGHT: f32 = 32.0;
const ROW_GAP: f32 = 1.0;
pub const ROW_PITCH: f32 = ROW_HEIGHT + ROW_GAP;
const CELL_GAP: f32 = 12.0;
const ICON_WIDTH: f32 = 16.0;

/// The rows of a table buffer from `first_line` on, under its path and sort controls.
pub fn render_table(
    table: Table,
    first_line: usize,
    point_line: usize,
    cx: &mut Context<EditorView>,
) -> impl IntoElement {
    let Table {
        crumbs,
        sort,
        columns,
        rows,
        activate,
        ..
    } = table;
    let rows = rows.into_iter().enumerate().map(|(index, row)| {
        let line = first_line + index;
        render_row(row, columns, line == point_line, line, activate, cx)
    });
    let rows: Vec<_> = rows.collect();
    div()
        .absolute()
        .top(px(LIST_TOP))
        .left(px(LIST_SIDE))
        .right(px(LIST_SIDE))
        .bottom(px(LIST_BOTTOM))
        .overflow_hidden()
        .flex()
        .flex_col()
        .gap(px(SECTION_GAP))
        .child(render_bar(crumbs, sort, cx))
        .child(render_header(columns))
        .child(div().flex().flex_col().gap(px(ROW_GAP)).children(rows))
}

/// Keys and what they do, centered under the rows.
pub fn render_hints(hints: &'static [(&'static str, &'static str)]) -> impl IntoElement {
    div()
        .absolute()
        .bottom_0()
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            pill()
                .relative()
                .font_family(font::UI)
                .children(hints.iter().map(|&(key, label)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .h(px(24.0))
                        .px(px(8.0))
                        .child(kbd(key, false))
                        .child(label)
                })),
        )
}

fn render_bar(crumbs: Vec<Button>, sort: Vec<Button>, cx: &mut Context<EditorView>) -> Div {
    let crumb_count = crumbs.len();
    let crumbs = crumbs.into_iter().enumerate().flat_map(|(index, crumb)| {
        let chevron = (index + 1 < crumb_count).then(|| {
            svg()
                .path(CHEVRON_ICON)
                .flex_none()
                .size(px(10.0))
                .text_color(rgb(color::FG_MUTED))
                .into_any_element()
        });
        [Some(render_crumb(crumb, cx)), chevron]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
    });
    let crumbs: Vec<AnyElement> = crumbs.collect();
    let options: Vec<AnyElement> = sort
        .into_iter()
        .map(|option| render_sort_option(option, cx))
        .collect();
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .h(px(BAR_HEIGHT))
        .pl(px(8.0))
        .pr(px(4.0))
        .child(div().flex().items_center().gap(px(2.0)).children(crumbs))
        .child(div().flex_1())
        .when(!options.is_empty(), |bar| {
            bar.child(
                div()
                    .font_family(font::UI)
                    .text_size(px(font::STATUS_SIZE))
                    .text_color(rgb(color::FG_FAINT))
                    .child("Sort"),
            )
            .child(
                div()
                    .flex()
                    .p(px(2.0))
                    .rounded(px(8.0))
                    .bg(rgb(color::BG_FIELD))
                    .border_1()
                    .border_color(rgba(0xffffff0f))
                    .children(options),
            )
        })
}

fn render_crumb(crumb: Button, cx: &mut Context<EditorView>) -> AnyElement {
    clickable(div(), crumb.action, cx)
        .flex()
        .items_center()
        .h(px(26.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .border_1()
        .border_color(rgba(0x00000000))
        .text_size(px(font::CODE_SIZE))
        .text_color(rgb(color::FG_DIM))
        .when(crumb.selected, |crumb| {
            crumb
                .bg(rgb(color::BG_POPUP))
                .border_color(rgba(0xffffff14))
                .text_color(rgb(color::FG_MAIN))
                .shadow(vec![shadow(0x00000099, 1.0, 2.0, 0.0)])
        })
        .child(crumb.label)
        .into_any_element()
}

fn render_sort_option(option: Button, cx: &mut Context<EditorView>) -> AnyElement {
    clickable(div(), option.action, cx)
        .flex()
        .items_center()
        .h(px(24.0))
        .px(px(10.0))
        .rounded(px(6.0))
        .font_family(font::UI)
        .text_size(px(font::STATUS_SIZE))
        .text_color(rgb(color::FG_DIM))
        .when(option.selected, |option| {
            option
                .bg(rgb(color::BG_KBD))
                .text_color(rgb(color::FG_MAIN))
                .shadow(vec![shadow(0x00000080, 1.0, 2.0, 0.0)])
        })
        .child(option.label)
        .into_any_element()
}

fn clickable(element: Div, action: Action, cx: &mut Context<EditorView>) -> Div {
    element.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |view, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            view.run_action(&action, cx);
        }),
    )
}

fn render_header(columns: &[Column]) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(CELL_GAP))
        .h(px(HEADER_HEIGHT))
        .px(px(12.0))
        .font_family(font::UI)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(font::SMALL_SIZE))
        .text_color(rgb(color::FG_FAINT))
        .child(div().flex_none().w(px(ICON_WIDTH)))
        .children(
            columns
                .iter()
                .map(|column| cell(column).child(column.title.to_uppercase())),
        )
}

fn render_row(
    row: Row,
    columns: &[Column],
    selected: bool,
    line: usize,
    activate: &'static str,
    cx: &mut Context<EditorView>,
) -> Div {
    let primary_color = match row.face {
        RowFace::Normal => color::FG_MAIN,
        RowFace::Directory => color::FG_DIRECTORY,
        RowFace::Dim => color::FG_FAINT,
    };
    let icon_color = match (selected, row.face) {
        (true, _) => color::FG_ALT,
        (false, RowFace::Directory) => color::FG_DIRECTORY,
        (false, _) => color::FG_FAINT,
    };
    let cells = row.cells.into_iter().zip(columns).map(|(text, column)| {
        let (family, size, text_color) = match column.style {
            CellStyle::Primary => (font::MONO, font::CODE_SIZE, primary_color),
            CellStyle::Figure => (font::MONO, font::STATUS_SIZE, color::FG_DIM),
            CellStyle::Label => (font::UI, font::STATUS_SIZE, color::FG_FAINT),
            CellStyle::Faint => (font::MONO, font::STATUS_SIZE, color::FG_LINE_NUMBER),
        };
        let text_color = if selected && column.style != CellStyle::Primary {
            color::FG_ALT
        } else {
            text_color
        };
        cell(column)
            .font_family(family)
            .text_size(px(size))
            .text_color(rgb(text_color))
            .child(text)
    });
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(CELL_GAP))
        .h(px(ROW_HEIGHT))
        .px(px(12.0))
        .rounded(px(8.0))
        .when(selected, |row| {
            row.bg(rgb(color::BG_SELECTION))
                .shadow(vec![shadow(0x00000073, 1.0, 3.0, 0.0)])
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                view.click_row(line, event.click_count, activate, cx);
            }),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .w(px(ICON_WIDTH))
                .children(row.icon.map(|row_icon| icon(row_icon, icon_color))),
        )
        .children(cells)
}

/// A column's slot: fixed and right-aligned, or the rest of the row, cut short.
fn cell(column: &Column) -> Div {
    match column.width {
        Some(width) => div()
            .flex_none()
            .w(px(width))
            .text_right()
            .whitespace_nowrap(),
        None => div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis(),
    }
}
