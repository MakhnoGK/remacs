use std::ops::Range;

use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Pixels, StyledText, div, prelude::*, px, rgb, rgba,
};
use remacs_core::{Action, Button, CellStyle, Column, Field, Mark, Notice, Row, RowFace, Table};
use remacs_theme::{color, font, glass};

use crate::assets::CHEVRON_ICON;
use crate::bottom_row;
use crate::composition::Composition;
use crate::display_line::DisplayLine;
use crate::editor_view::EditorView;
use crate::highlights::region_style;
use crate::preview_view::PREVIEW_WIDTH;
use crate::widgets::{
    PressExt, Scrolled, StyleExt, fade_scrolled, icon, kbd, mark_badge, modified_dot, scrolled_by,
    segment, segmented, shadow, state_letter, svg_icon, text_with_cursor,
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
/// How far the selected row's shadow reaches past it (gpui blurs 3× its radius, plus its
/// offset); the clipping boxes grow by it.
const SHADOW_ROOM: f32 = 10.0;
const PREVIEW_GAP: f32 = 20.0;
/// Narrower, the names would be cut short to make room for a preview.
const MIN_LIST_WIDTH: f32 = 520.0;

/// Point and the region, for rows whose first cell is a field, and the text being
/// composed when that field takes typed text.
pub struct TableCursor<'a> {
    pub line: usize,
    pub column: usize,
    /// The columns of each row's text in the region, from the first row on.
    pub regions: Vec<Option<Range<usize>>>,
    pub composition: Option<&'a Composition>,
}

/// The rows of a table buffer from `first_line` on, `scrolled_past` of the first row
/// hidden, under its path and sort controls, with the selected row's preview, or the
/// pending changes, beside them.
pub fn render_table(
    table: Table,
    first_line: usize,
    scrolled: Scrolled,
    scrolled_past: f32,
    cursor: TableCursor<'_>,
    side: Option<Div>,
    cx: &mut Context<EditorView>,
) -> impl IntoElement + use<> {
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
        composition,
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
                composition,
            };
            render_row(row, columns, at_point, line, activate, cx)
        });
    let rows = scrolled_by(
        div().flex().flex_col().gap(px(ROW_GAP)).children(rows),
        ROW_PITCH * scrolled_past,
    )
    .m(px(-SHADOW_ROOM))
    .p(px(SHADOW_ROOM));
    let list = div()
        .flex_1()
        .min_w_0()
        .m(px(-SHADOW_ROOM))
        .p(px(SHADOW_ROOM))
        .overflow_hidden()
        .flex()
        .flex_col()
        .gap(px(SECTION_GAP))
        .child(render_bar(crumbs, sort, notice, cx))
        .child(render_header(columns))
        .child(fade_scrolled(
            rows,
            scrolled,
            rgba(glass::window_tint()).into(),
        ));
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

/// Taken right of the rows by a preview or pending changes beside them, which the bottom
/// row keeps clear of.
pub const SIDE_ROOM: f32 = PREVIEW_GAP + PREVIEW_WIDTH;

/// `side` beside the rows going down past the room they leave for the bottom row, as far
/// as the row, which keeps clear of it; `rise` higher while the row, on its way as the
/// pane comes or goes, stands over where the rows end, lower while under it.
pub fn down_to_the_row(side: Div, rise: Pixels) -> Div {
    side.mb(px(bottom_row::BOTTOM - LIST_BOTTOM) + rise)
}

/// Whether a window this wide has room for a preview beside the rows.
pub fn fits_preview(window_width: Pixels) -> bool {
    window_width >= px(MIN_LIST_WIDTH + PREVIEW_GAP + PREVIEW_WIDTH + 2.0 * LIST_SIDE)
}

fn render_bar(
    crumbs: Vec<Button>,
    sort: Vec<Button>,
    notice: Option<Notice>,
    cx: &mut Context<EditorView>,
) -> Div {
    let crumb_count = crumbs.len();
    let crumbs: Vec<AnyElement> = crumbs
        .into_iter()
        .enumerate()
        .flat_map(|(index, crumb)| {
            let chevron = (index + 1 < crumb_count).then(|| {
                svg_icon(CHEVRON_ICON, 10.0, color::fg_muted())
                    .flex_none()
                    .into_any_element()
            });
            [Some(render_crumb(crumb, cx)), chevron]
        })
        .flatten()
        .collect();
    let options: Vec<Div> = sort
        .into_iter()
        .map(|option| clickable(segment(&option.label, option.selected), option.action, cx))
        .collect();
    let sort_label = div().ui().status_text(color::fg_faint()).child("Sort");
    div()
        .row(12.0)
        .h(px(BAR_HEIGHT))
        .pl(px(8.0))
        .pr(px(4.0))
        .child(div().row(2.0).min_w_0().children(crumbs))
        .child(div().flex_1())
        .when(!options.is_empty(), |bar| {
            bar.child(sort_label).child(segmented().children(options))
        })
        .children(notice.map(render_notice))
}

/// What the rows are being edited for, with the keys that end it, on a warning tint.
fn render_notice(notice: Notice) -> Div {
    let notice_kbd = |key| {
        kbd(key, false)
            .bg(rgba(glass::field()))
            .text_color(rgb(color::fg_on_warning()))
    };
    div()
        .row(8.0)
        .flex_none()
        .h(px(28.0))
        .pl(px(10.0))
        .pr(px(6.0))
        .rounded_fill(8.0, color::warning() << 8 | 0x1a)
        .bordered(color::warning() << 8 | 0x4d)
        .ui()
        .status_text(color::fg_on_warning())
        .child(modified_dot())
        .child(notice.label)
        .children(notice.keys.iter().map(|&(keys, label)| {
            div()
                .row(6.0)
                .ml(px(4.0))
                .children(keys.split_whitespace().map(notice_kbd))
                .child(label)
        }))
}

fn render_crumb(crumb: Button, cx: &mut Context<EditorView>) -> AnyElement {
    let selected = crumb.selected;
    clickable(div(), crumb.action, cx)
        .row(0.0)
        .bar(26.0, 8.0)
        .rounded(px(6.0))
        .bordered(0x00000000)
        .text(font::mono_size(), color::fg_dim())
        // On a long path the outer directories give way, so the sort controls stay.
        .when(!selected, |element| element.min_w_0())
        .when(selected, |element| {
            element
                .flex_none()
                .bg(rgba(glass::control()))
                .border_color(rgba(glass::control_border()))
                .text_color(rgb(color::fg_main()))
                .shadow(vec![shadow(glass::shadow(0x99), 1.0, 2.0, 0.0)])
        })
        .child(div().min_w_0().truncate().child(crumb.label))
        .into_any_element()
}

pub fn clickable(element: Div, action: Action, cx: &mut Context<EditorView>) -> Div {
    element.on_press(cx, move |view, cx| view.run_action(&action, cx))
}

fn render_header(columns: &[Column]) -> Div {
    div()
        .row(CELL_GAP)
        .bar(HEADER_HEIGHT, 12.0)
        .ui()
        .font_weight(FontWeight::SEMIBOLD)
        .text(font::small_size(), color::fg_faint())
        .child(div().flex_none().w(px(MARK_WIDTH)))
        .child(div().flex_none().w(px(ICON_WIDTH)))
        .children(
            columns
                .iter()
                .map(|column| cell(column).child(column.title.to_uppercase())),
        )
}

/// Whether a row is point's, and the part of it to draw as the cursor and the region.
struct RowCursor<'a> {
    selected: bool,
    column: usize,
    region: Option<Range<usize>>,
    composition: Option<&'a Composition>,
}

fn render_row(
    row: Row,
    columns: &[Column],
    cursor: RowCursor<'_>,
    line: usize,
    activate: &'static str,
    cx: &mut Context<EditorView>,
) -> Div {
    let selected = cursor.selected;
    // An edited row's selection is its focused field.
    let highlighted = selected && row.field.is_none();
    let flagged = row.mark == Some(Mark::Flagged);
    let primary_color = match row.face {
        _ if flagged => color::fg_flagged(),
        RowFace::Normal => color::fg_main(),
        RowFace::Directory => color::fg_directory(),
        RowFace::Dim => color::fg_faint(),
    };
    let mark_tint = row.mark.map(|mark| match mark {
        Mark::Marked => color::accent(),
        Mark::Flagged => color::error(),
    });
    let icon_color = match (selected, row.face) {
        (true, _) => color::fg_alt(),
        (false, RowFace::Directory) => color::fg_directory(),
        (false, _) => color::fg_faint(),
    };
    let mut field = row.field;
    let state = row.state;
    let cells = row.cells.into_iter().zip(columns).map(|(text, column)| {
        let primary = column.style == CellStyle::Primary;
        let (family, size, text_color) = match column.style {
            CellStyle::Primary => (font::mono(), font::mono_size(), primary_color),
            CellStyle::Figure => (font::mono(), font::status_size(), color::fg_dim()),
            CellStyle::Label => (font::ui(), font::status_size(), color::fg_faint()),
            CellStyle::Faint => (font::mono(), font::status_size(), color::fg_line_number()),
            CellStyle::State => {
                return state_letter(state, highlighted).w(px(column.width.unwrap_or_default()));
            }
        };
        let text_color = if highlighted && !primary {
            color::fg_alt()
        } else {
            text_color
        };
        if primary && let Some(field) = field.take() {
            let at_point = selected.then_some(cursor.column);
            let composition = cursor.composition.filter(|_| selected);
            let region = cursor.region.clone();
            return render_field(text, field, text_color, at_point, region, composition);
        }
        cell(column)
            .font_family(family)
            .text(size, text_color)
            .when(flagged && primary, Styled::line_through)
            .child(text)
    });
    let cells: Vec<_> = cells.collect();
    div()
        .row(CELL_GAP)
        .flex_none()
        .bar(ROW_HEIGHT, 12.0)
        .rounded(px(8.0))
        .when_some(mark_tint.filter(|_| !highlighted), |row, tint| {
            row.bg(rgba(tint << 8 | 0x14))
        })
        .when(highlighted, |row| {
            row.bg(rgba(glass::selection())).shadow(vec![shadow(
                glass::shadow(0x73),
                1.0,
                3.0,
                0.0,
            )])
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
    composition: Option<&Composition>,
) -> Div {
    let mut display = DisplayLine::new(&text);
    let highlights = region
        .map(|columns| vec![(display.cell_range(columns), region_style())])
        .unwrap_or_default();
    let content = match at_point {
        Some(column) => {
            let cursor = display.cursor_cell(column);
            text_with_cursor(&display.text, &highlights, cursor, composition)
        }
        None => div().child(StyledText::new(display.text).with_highlights(highlights)),
    };
    let input = div()
        .row(0.0)
        .flex_1()
        .min_w_0()
        .h(px(24.0))
        .ml(px(-7.0))
        .px(px(6.0))
        .overflow_hidden()
        .whitespace_nowrap()
        .rounded_fill(6.0, glass::field())
        .bordered(glass::ink(0x14))
        .mono()
        .text(font::mono_size(), text_color)
        .when(field.original.is_some(), |input| {
            input
                .border_color(rgba(color::warning() << 8 | 0x80))
                .bg(rgba(color::warning() << 8 | 0x14))
        })
        .when(at_point.is_some(), |input| {
            input
                .border_color(rgba(color::accent() << 8 | 0xbf))
                .shadow(vec![shadow(color::accent() << 8 | 0x33, 0.0, 0.0, 3.0)])
        })
        .child(content);
    div()
        .row(CELL_GAP)
        .flex_1()
        .min_w_0()
        .child(input)
        .children(field.original.map(|original| {
            div()
                .flex_none()
                .ui()
                .status_text(color::fg_faint())
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
