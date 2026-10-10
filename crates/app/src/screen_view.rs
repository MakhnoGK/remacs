//! A buffer drawn as a terminal's screen: runs of cells at their columns in the code
//! font, the cursor, the marks of commands in a gutter, and what is over the rows.

use gpui::{
    Context, Div, FontWeight, Hsla, ParentElement, Pixels, Styled, div, prelude::*, px, rgb, rgba,
};
use remacs_core::{
    CellFace, CellHighlight, CellRun, CursorShape, RowMark, Screen, ScreenBanner, ScreenCursor,
    ScreenNotice, ScreenRow, ScreenSearch, ScreenSize, Tone,
};
use remacs_theme::{color, font, glass};

use crate::editor_view::EditorView;
use crate::highlights::console_color;
use crate::widgets::{Edge, edge_fade, glass_surface, hsla, key_caps, tone_color};

/// Room left of the rows for the marks of commands, once a shell reports them.
pub const MARKS_GUTTER: f32 = 40.0;
const SEARCH_BAR_WIDTH: f32 = 560.0;
const SEARCH_BAR_HEIGHT: f32 = 44.0;

/// Measured from the code font.
#[derive(Clone, Copy)]
pub struct Cell {
    pub width: Pixels,
    pub height: Pixels,
    /// Ascent and descent, what a block cursor covers.
    pub glyph: Pixels,
}

/// The cells that fit in `width` × `height`.
pub fn screen_size(width: Pixels, height: Pixels, cell: Cell, gutter: bool) -> ScreenSize {
    let width = width - if gutter { px(MARKS_GUTTER) } else { px(0.0) };
    ScreenSize {
        columns: (width / cell.width).floor().max(1.0) as usize,
        rows: (height / cell.height).floor().max(1.0) as usize,
    }
}

/// The cell under a point `x`, `y` from the screen's top left, gutter included.
pub fn cell_at(x: Pixels, y: Pixels, cell: Cell, gutter: bool) -> (usize, usize) {
    let x = x - if gutter { px(MARKS_GUTTER) } else { px(0.0) };
    let column = (x / cell.width).floor().max(0.0) as usize;
    let row = (y / cell.height).floor().max(0.0) as usize;
    (row, column)
}

/// The screen filling its parent; `fade` dissolves its top into the glass, as scrolled
/// text does. `hovered` is the row whose mark the mouse is over.
pub fn render_screen(
    screen: Screen,
    cell: Cell,
    fade: f32,
    hovered: Option<usize>,
    cx: &mut Context<EditorView>,
) -> Div {
    let Screen {
        rows,
        cursor,
        banner,
        notice,
        mode: _,
        search,
        gutter,
        above: _,
        flash,
    } = screen;
    let root = div().relative().size_full().overflow_hidden();
    if let Some(notice) = notice {
        return root.child(render_notice(notice));
    }
    let left = if gutter { px(MARKS_GUTTER) } else { px(0.0) };
    let cells = div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(left)
        .right_0()
        .font_family(font::mono())
        .text_size(px(font::code().size))
        .line_height(cell.height)
        .text_color(rgb(color::FG_BODY))
        .children(
            rows.iter()
                .enumerate()
                .flat_map(|(row, line)| render_row(row, line, cell)),
        )
        .children(cursor.map(|cursor| render_cursor(cursor, &rows, cell)));
    let glass = rgba(glass::window_tint()).into();
    root.when(gutter, |root| {
        root.children(render_marks(&rows, cell, hovered, cx))
    })
    .child(cells)
    .when(fade > 0.0, |root| {
        root.child(edge_fade(Edge::Top, fade, glass))
    })
    .children(banner.map(render_banner))
    .children(search.map(render_search))
    .when(flash, |root| {
        root.child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(10.0))
                .bg(rgba(0xffffff0f))
                .border_1()
                .border_color(rgba(color::WARNING << 8 | 0x66)),
        )
    })
}

fn render_row(row: usize, line: &ScreenRow, cell: Cell) -> Vec<Div> {
    let top = cell.height * row as f32;
    let mut column = 0;
    let mut drawn = Vec::with_capacity(line.runs.len());
    for run in &line.runs {
        let left = cell.width * column as f32;
        column += run.cells;
        if run.text.trim().is_empty()
            && run.face.background.is_none()
            && run.face.highlight.is_none()
        {
            continue;
        }
        drawn.push(render_run(run, left, top, cell));
    }
    drawn
}

fn render_run(run: &CellRun, left: Pixels, top: Pixels, cell: Cell) -> Div {
    let (text, background) = colors(&run.face);
    let face = run.face;
    div()
        .absolute()
        .left(left)
        .top(top)
        .w(cell.width * run.cells as f32)
        .h(cell.height)
        .whitespace_nowrap()
        .text_color(text)
        .when_some(background, |run, background| run.bg(background))
        .when(face.bold, |run| run.font_weight(FontWeight::BOLD))
        .when(face.italic, |run| run.italic())
        .when(face.underline, |run| {
            run.text_decoration_1().text_decoration_color(text)
        })
        .when(face.strikeout, |run| run.line_through())
        .child(run.text.clone())
}

/// The text's color and the background's, the highlight over the cell's own.
fn colors(face: &CellFace) -> (Hsla, Option<Hsla>) {
    let foreground = face.foreground.map_or(color::FG_BODY, console_color);
    let mut text = hsla(foreground);
    if face.dim {
        text.a = 0.6;
    }
    let background = face
        .background
        .map(|background| hsla(console_color(background)));
    match face.highlight {
        None => (text, background),
        Some(CellHighlight::Selection) => (text, Some(rgba(glass::SELECTION).into())),
        Some(CellHighlight::SearchMatch) => (
            hsla(color::FG_ON_WARNING),
            Some(rgba(color::WARNING << 8 | 0x3d).into()),
        ),
        Some(CellHighlight::SearchCurrent) => (hsla(color::BG_MAIN), Some(hsla(color::WARNING))),
        Some(CellHighlight::Link) => (hsla(color::LINK), background),
    }
}

fn render_cursor(cursor: ScreenCursor, rows: &[ScreenRow], cell: Cell) -> Div {
    let left = cell.width * cursor.column as f32;
    let top = cell.height * cursor.row as f32;
    let glyph_top = top + (cell.height - cell.glyph) / 2.0;
    let quad = div().absolute().left(left);
    match cursor.shape {
        CursorShape::Block => {
            let under = character_at(rows, cursor).unwrap_or_default();
            quad.top(glyph_top)
                .w(cell.width)
                .h(cell.glyph)
                .rounded(px(1.0))
                .bg(rgb(color::FG_MAIN))
                .text_color(rgb(color::BG_MAIN))
                .flex()
                .items_center()
                .child(under)
        }
        CursorShape::Bar => quad
            .top(glyph_top)
            .w(px(2.0))
            .h(cell.glyph)
            .bg(rgb(color::FG_MAIN)),
        CursorShape::Underline => quad
            .top(glyph_top + cell.glyph - px(2.0))
            .w(cell.width)
            .h(px(2.0))
            .bg(rgb(color::FG_MAIN)),
        CursorShape::Hollow => quad
            .top(glyph_top)
            .w(cell.width)
            .h(cell.glyph)
            .rounded(px(1.0))
            .border_1()
            .border_color(rgba(color::FG_MAIN << 8 | 0x99)),
    }
}

/// The character in the cell under a cursor, drawn inverted over it.
fn character_at(rows: &[ScreenRow], cursor: ScreenCursor) -> Option<String> {
    let mut column = 0;
    for run in &rows.get(cursor.row)?.runs {
        if cursor.column < column + run.cells {
            let index = if run.cells == run.text.chars().count() {
                cursor.column - column
            } else {
                0
            };
            return run.text.chars().nth(index).map(String::from);
        }
        column += run.cells;
    }
    None
}

/// ✓ or ✗ with the exit code beside each command's prompt, and a bar down its output.
fn render_marks(
    rows: &[ScreenRow],
    cell: Cell,
    hovered: Option<usize>,
    cx: &mut Context<EditorView>,
) -> Vec<Div> {
    let mut marks = Vec::new();
    for (row, line) in rows.iter().enumerate() {
        let Some(mark) = &line.mark else {
            continue;
        };
        let end = rows[row + 1..]
            .iter()
            .position(|below| below.prompt)
            .map_or(rows.len(), |offset| row + 1 + offset);
        marks.push(render_mark(row, end, mark, cell, hovered == Some(row), cx));
    }
    marks
}

fn render_mark(
    row: usize,
    end: usize,
    mark: &RowMark,
    cell: Cell,
    hovered: bool,
    cx: &mut Context<EditorView>,
) -> Div {
    let top = cell.height * row as f32;
    let (glyph, tone) = match mark.exit {
        None => ("", color::FG_DIM),
        Some(0) => ("✓", color::VC_ADDED),
        Some(_) => ("✗", color::ERROR),
    };
    let badge = div()
        .id(("command-mark", row))
        .absolute()
        .left(px(7.0))
        .top(top + px(4.0))
        .flex()
        .items_center()
        .justify_center()
        .w(px(16.0))
        .h(px(14.0))
        .rounded(px(5.0))
        .font_family(font::UI)
        .font_weight(FontWeight::BOLD)
        .text_size(px(10.0))
        .text_color(rgb(tone))
        .map(|badge| match mark.exit {
            None => badge
                .size(px(10.0))
                .left(px(10.0))
                .top(top + px(6.0))
                .rounded_full()
                .border_1()
                .border_color(rgba(0xffffff4d)),
            Some(_) => badge.bg(rgba(tone << 8 | 0x2e)),
        })
        .child(glyph)
        .on_hover(cx.listener(move |view, hovered: &bool, _, cx| {
            view.hover_command_mark(hovered.then_some(row), cx);
        }));
    let bar_top = top + px(18.0);
    let bar_bottom = cell.height * end as f32 - px(5.0);
    let bar = (bar_bottom > bar_top).then(|| {
        div()
            .absolute()
            .left(px(14.0))
            .top(bar_top)
            .w(px(2.0))
            .h(bar_bottom - bar_top)
            .rounded(px(1.0))
            .bg(rgba(tone << 8 | 0x73))
    });
    let code = mark.exit.filter(|&code| code != 0).map(|code| {
        div()
            .absolute()
            .left(px(25.0))
            .top(top + px(6.0))
            .font_family(font::mono())
            .text_size(px(9.0))
            .line_height(px(10.0))
            .text_color(rgb(color::DANGER))
            .child(code.to_string())
    });
    let tip = hovered.then(|| {
        glass_surface(9.0)
            .absolute()
            .left(px(MARKS_GUTTER))
            .top(top - px(34.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .h(px(30.0))
            .px(px(10.0))
            .whitespace_nowrap()
            .font_family(font::mono())
            .text_size(px(font::STATUS_SIZE))
            .text_color(rgb(color::FG_BODY))
            .child(mark.detail.clone())
    });
    div()
        .children(bar)
        .child(badge)
        .children(code)
        .children(tip)
}

/// The shell starting, or why it could not, in place of the rows.
fn render_notice(notice: ScreenNotice) -> Div {
    let accent = tone_color(notice.tone, color::FG_DIM);
    let danger = notice.tone == Tone::Danger;
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .max_w(px(560.0))
                .p(px(18.0))
                .rounded(px(12.0))
                .when(danger, |card| {
                    card.bg(rgba(color::ERROR << 8 | 0x14))
                        .border_1()
                        .border_color(rgba(color::ERROR << 8 | 0x59))
                })
                .font_family(font::UI)
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(font::UI_SIZE + 2.0))
                        .text_color(rgb(if danger { accent } else { color::FG_MAIN }))
                        .child(notice.title),
                )
                .child(
                    div()
                        .font_family(font::mono())
                        .text_size(px(font::STATUS_SIZE))
                        .line_height(px(18.0))
                        .text_color(rgb(color::FG_DIM))
                        .child(notice.detail),
                ),
        )
}

/// That the program ended, over the bottom of the rows.
fn render_banner(banner: ScreenBanner) -> Div {
    let tone = tone_color(banner.tone, color::FG_DIM);
    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom(px(8.0))
        .flex()
        .justify_center()
        .child(
            glass_surface(10.0)
                .flex()
                .items_center()
                .gap(px(10.0))
                .h(px(34.0))
                .px(px(12.0))
                .font_family(font::UI)
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_BODY))
                .child(div().size(px(8.0)).rounded_full().bg(rgb(tone)))
                .child(banner.text)
                .children(banner.keys.iter().map(|&(keys, label)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .text_color(rgb(color::FG_DIM))
                        .children(key_caps(keys))
                        .child(label)
                })),
        )
}

/// `I-search:` and what is typed, with the match shown and how many there are.
fn render_search(search: ScreenSearch) -> Div {
    let found = match search.found {
        Some((current, total)) => format!("{current} of {total}"),
        None if search.input.is_empty() => String::new(),
        None => "no match".to_owned(),
    };
    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom(px(10.0))
        .flex()
        .justify_center()
        .child(
            glass_surface(14.0)
                .flex()
                .items_center()
                .gap(px(10.0))
                .w(px(SEARCH_BAR_WIDTH))
                .h(px(SEARCH_BAR_HEIGHT))
                .pl(px(14.0))
                .pr(px(12.0))
                .font_family(font::UI)
                .text_size(px(font::UI_SIZE))
                .child(div().text_color(rgb(color::FG_DIM)).child("I-search:"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .font_family(font::mono())
                        .text_color(rgb(color::FG_MAIN))
                        .child(search.input)
                        .child(
                            div()
                                .ml(px(1.0))
                                .w(px(8.0))
                                .h(px(17.0))
                                .rounded(px(1.0))
                                .bg(rgb(color::FG_MAIN)),
                        ),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .font_family(font::mono())
                        .text_size(px(font::STATUS_SIZE))
                        .text_color(rgb(color::WARNING))
                        .child(found),
                )
                .children(search.hints.iter().map(|&(keys, label)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .text_size(px(font::STATUS_SIZE))
                        .text_color(rgb(color::FG_DIM))
                        .children(key_caps(keys))
                        .child(label)
                })),
        )
}
