use std::ops::Range;

use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Pixels, StyledText, div, prelude::*, px, rgb, rgba, svg,
};
use remacs_core::{Action, Button, CellStyle, Column, Field, Mark, Notice, Row, RowFace, Table};
use remacs_theme::{color, font};

use crate::assets::CHEVRON_ICON;
use crate::display_line::DisplayLine;
use crate::editor_view::EditorView;
use crate::highlights::{combine, region_style};
use crate::preview_view::PREVIEW_WIDTH;
use crate::widgets::{
    cursor_style, icon, kbd, key_caps, mark_badge, modified_dot, pill, segment, segmented, shadow,
    state_letter,
};

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
const MARK_WIDTH: f32 = 18.0;
const PREVIEW_GAP: f32 = 20.0;
/// Narrower, the names would be cut short to make room for a preview.
const MIN_LIST_WIDTH: f32 = 520.0;

/// Point and the region, for rows whose first cell is a field.
pub struct TableCursor {
    pub line: usize,
    pub column: usize,
    /// The columns of each row's text in the region, from the first row on.
    pub regions: Vec<Option<Range<usize>>>,
}

/// The rows of a table buffer from `first_line` on, under its path and sort controls, with
/// the selected row's preview, or the pending changes, beside them.
pub fn render_table(
    table: Table,
    first_line: usize,
    cursor: TableCursor,
    side: Option<Div>,
    cx: &mut Context<EditorView>,
) -> impl IntoElement {
    let Table {
        crumbs,
        sort,
        columns,
        rows,
        activate,
        notice,
        ..
    } = table;
    let TableCursor {
        line: point_line,
        column: point_column,
        mut regions,
    } = cursor;
    regions.resize(rows.len(), None);
    let rows = rows
        .into_iter()
        .zip(regions)
        .enumerate()
        .map(|(index, (row, region))| {
            let line = first_line + index;
            let at_point = RowCursor {
                selected: line == point_line,
                column: point_column,
                region,
            };
            render_row(row, columns, at_point, line, activate, cx)
        });
    let rows: Vec<_> = rows.collect();
    let list = div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_col()
        .gap(px(SECTION_GAP))
        .child(render_bar(crumbs, sort, notice, cx))
        .child(render_header(columns))
        .child(div().flex().flex_col().gap(px(ROW_GAP)).children(rows));
    div()
        .absolute()
        .top(px(LIST_TOP))
        .left(px(LIST_SIDE))
        .right(px(LIST_SIDE))
        .bottom(px(LIST_BOTTOM))
        .flex()
        .gap(px(PREVIEW_GAP))
        .child(list)
        .children(side)
}

/// Whether a window this wide has room for a preview beside the rows.
pub fn fits_preview(window_width: Pixels) -> bool {
    window_width >= px(MIN_LIST_WIDTH + PREVIEW_GAP + PREVIEW_WIDTH + 2.0 * LIST_SIDE)
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
                .children(hints.iter().map(|&(keys, label)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .h(px(24.0))
                        .px(px(8.0))
                        .children(key_caps(keys))
                        .child(label)
                })),
        )
}

fn render_bar(
    crumbs: Vec<Button>,
    sort: Vec<Button>,
    notice: Option<Notice>,
    cx: &mut Context<EditorView>,
) -> Div {
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
        .child(
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(px(2.0))
                .children(crumbs),
        )
        .child(div().flex_1())
        .when(!options.is_empty(), |bar| {
            bar.child(
                div()
                    .font_family(font::UI)
                    .text_size(px(font::STATUS_SIZE))
                    .text_color(rgb(color::FG_FAINT))
                    .child("Sort"),
            )
            .child(segmented().children(options))
        })
        .children(notice.map(render_notice))
}

/// What the rows are being edited for, with the keys that end it, on a warning tint.
fn render_notice(notice: Notice) -> Div {
    let notice_kbd = |key| {
        kbd(key, false)
            .bg(rgba(0x00000059))
            .text_color(rgb(color::FG_ON_WARNING))
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .h(px(28.0))
        .pl(px(10.0))
        .pr(px(6.0))
        .rounded(px(8.0))
        .bg(rgba(color::WARNING << 8 | 0x1a))
        .border_1()
        .border_color(rgba(color::WARNING << 8 | 0x4d))
        .font_family(font::UI)
        .text_size(px(font::STATUS_SIZE))
        .text_color(rgb(color::FG_ON_WARNING))
        .child(modified_dot())
        .child(notice.label)
        .children(notice.keys.iter().map(|&(keys, label)| {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .ml(px(4.0))
                .children(keys.split_whitespace().map(notice_kbd))
                .child(label)
        }))
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
        // On a long path the outer directories give way, so the sort controls stay.
        .map(|element| {
            if crumb.selected {
                element.flex_none()
            } else {
                element.min_w_0()
            }
        })
        .child(div().min_w_0().truncate().child(crumb.label))
        .into_any_element()
}

fn render_sort_option(option: Button, cx: &mut Context<EditorView>) -> AnyElement {
    clickable(segment(&option.label, option.selected), option.action, cx).into_any_element()
}

pub fn clickable(element: Div, action: Action, cx: &mut Context<EditorView>) -> Div {
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
        .child(div().flex_none().w(px(MARK_WIDTH)))
        .child(div().flex_none().w(px(ICON_WIDTH)))
        .children(
            columns
                .iter()
                .map(|column| cell(column).child(column.title.to_uppercase())),
        )
}

/// Whether a row is point's, and the part of it to draw as the cursor and the region.
struct RowCursor {
    selected: bool,
    column: usize,
    region: Option<Range<usize>>,
}

fn render_row(
    row: Row,
    columns: &[Column],
    cursor: RowCursor,
    line: usize,
    activate: &'static str,
    cx: &mut Context<EditorView>,
) -> Div {
    let selected = cursor.selected;
    // An edited row's selection is its focused field.
    let highlighted = selected && row.field.is_none();
    let flagged = row.mark == Some(Mark::Flagged);
    let primary_color = match row.face {
        _ if flagged => color::FG_FLAGGED,
        RowFace::Normal => color::FG_MAIN,
        RowFace::Directory => color::FG_DIRECTORY,
        RowFace::Dim => color::FG_FAINT,
    };
    let mark_tint = row.mark.map(|mark| match mark {
        Mark::Marked => color::ACCENT,
        Mark::Flagged => color::ERROR,
    });
    let icon_color = match (selected, row.face) {
        (true, _) => color::FG_ALT,
        (false, RowFace::Directory) => color::FG_DIRECTORY,
        (false, _) => color::FG_FAINT,
    };
    let mut field = row.field;
    let state = row.state;
    let cells = row.cells.into_iter().zip(columns).map(|(text, column)| {
        let (family, size, text_color) = match column.style {
            CellStyle::Primary => (font::MONO, font::CODE_SIZE, primary_color),
            CellStyle::Figure => (font::MONO, font::STATUS_SIZE, color::FG_DIM),
            CellStyle::Label => (font::UI, font::STATUS_SIZE, color::FG_FAINT),
            CellStyle::Faint => (font::MONO, font::STATUS_SIZE, color::FG_LINE_NUMBER),
            CellStyle::State => {
                return state_letter(state, highlighted).w(px(column.width.unwrap_or_default()));
            }
        };
        let text_color = if highlighted && column.style != CellStyle::Primary {
            color::FG_ALT
        } else {
            text_color
        };
        if column.style == CellStyle::Primary
            && let Some(field) = field.take()
        {
            let at_point = selected.then_some(cursor.column);
            return render_field(text, field, text_color, at_point, cursor.region.clone());
        }
        cell(column)
            .font_family(family)
            .text_size(px(size))
            .text_color(rgb(text_color))
            .when(
                flagged && column.style == CellStyle::Primary,
                Styled::line_through,
            )
            .child(text)
    });
    let cells: Vec<_> = cells.collect();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(CELL_GAP))
        .h(px(ROW_HEIGHT))
        .px(px(12.0))
        .rounded(px(8.0))
        .when_some(mark_tint.filter(|_| !highlighted), |row, tint| {
            row.bg(rgba(tint << 8 | 0x14))
        })
        .when(highlighted, |row| {
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
                .justify_center()
                .w(px(MARK_WIDTH))
                .children(row.mark.map(mark_badge)),
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

/// The first cell as a text field, tinted once edited and lit with the cursor at
/// `at_point`, followed by what it said before.
fn render_field(
    text: String,
    field: Field,
    text_color: u32,
    at_point: Option<usize>,
    region: Option<Range<usize>>,
) -> Div {
    let mut display = DisplayLine::new(&text);
    let mut layers = Vec::new();
    if let Some(columns) = region {
        layers.push(vec![(display.cell_range(columns), region_style())]);
    }
    if let Some(column) = at_point {
        layers.push(vec![(display.cursor_cell(column), cursor_style())]);
    }
    let content = StyledText::new(display.text).with_highlights(combine(&layers));
    let edited = field.original.is_some();
    let input = div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .h(px(24.0))
        .ml(px(-7.0))
        .px(px(6.0))
        .overflow_hidden()
        .whitespace_nowrap()
        .rounded(px(6.0))
        .border_1()
        .border_color(rgba(0xffffff14))
        .bg(rgb(color::BG_PANE))
        .font_family(font::MONO)
        .text_size(px(font::CODE_SIZE))
        .text_color(rgb(text_color))
        .when(edited, |input| {
            input
                .border_color(rgba(color::WARNING << 8 | 0x80))
                .bg(rgba(color::WARNING << 8 | 0x14))
        })
        .when(at_point.is_some(), |input| {
            input
                .border_color(rgba(color::ACCENT << 8 | 0xbf))
                .shadow(vec![shadow(color::ACCENT << 8 | 0x33, 0.0, 0.0, 3.0)])
        })
        .child(content);
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(px(CELL_GAP))
        .child(input)
        .children(field.original.map(|original| {
            div()
                .flex_none()
                .font_family(font::UI)
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_FAINT))
                .child(format!("was {original}"))
        }))
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
