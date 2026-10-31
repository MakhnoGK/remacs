//! A buffer drawn as a terminal's screen: runs of cells at their columns in the code
//! font, the cursor, the marks of commands, and what is over the rows.

use gpui::{
    BorderStyle, Bounds, Context, Corner, Corners, Div, Edges, FontWeight, Hsla, ParentElement,
    PathBuilder, Pixels, Point, Styled, Window, canvas, div, fill, prelude::*, px, quad, rgb, rgba,
    transparent_black,
};
use remacs_core::{
    CellFace, CellHighlight, CellRun, CursorShape, RowMark, Screen, ScreenBanner, ScreenCursor,
    ScreenNotice, ScreenRow, ScreenSearch, ScreenSize, Tone,
};
use remacs_theme::{color, cursor, font, glass};

use crate::cell_glyphs::{self, Piece};
use crate::editor_view::EditorView;
use crate::highlights::console_color;
use crate::transition::Look;
use crate::widgets::{
    Edge, StyleExt, block_cursor, bottom_centered, cursor_height, edge_fade, glass_surface, hsla,
    inline_cursor, key_hint, tone_color,
};

/// The edge down a command's rows, in the screen's left padding.
const EDGE_WIDTH: f32 = 3.0;
/// The least the mouse finds the edge within.
const EDGE_REACH: f32 = 6.0;
const PLATE_HEIGHT: f32 = 16.0;
const TIP_HEIGHT: f32 = 30.0;
const SEARCH_BAR_WIDTH: f32 = 560.0;
const SEARCH_BAR_HEIGHT: f32 = 44.0;

/// Measured from the code font.
#[derive(Clone, Copy)]
pub struct Cell {
    /// Of the code font, as zoomed for screens.
    pub font_size: Pixels,
    pub width: Pixels,
    pub height: Pixels,
    /// Ascent and descent, what a block cursor covers.
    pub glyph: Pixels,
}

/// The cells that fit in `width` × `height`.
pub fn screen_size(width: Pixels, height: Pixels, cell: Cell) -> ScreenSize {
    ScreenSize {
        columns: (width / cell.width).floor().max(1.0) as usize,
        rows: (height / cell.height).floor().max(1.0) as usize,
    }
}

/// The cell under a point `x`, `y` from the screen's top left.
pub fn cell_at(x: Pixels, y: Pixels, cell: Cell) -> (usize, usize) {
    let column = (x / cell.width).floor().max(0.0) as usize;
    let row = (y / cell.height).floor().max(0.0) as usize;
    (row, column)
}

/// The screen filling its parent, its cells `inset` from the edges. With more scrolled
/// past above, its top, inset included, dissolves into `under`, what it is drawn on, as
/// scrolled text does. `tip` is the row whose mark the mouse is or was over, whether over
/// the mark's plate rather than its edge, and how its tooltip comes or goes.
pub fn render_screen(
    screen: Screen,
    cell: Cell,
    inset: Point<Pixels>,
    under: Hsla,
    tip: Option<((usize, bool), Look)>,
    cx: &mut Context<EditorView>,
) -> Div {
    let Screen { rows, cursor, banner, notice, mode: _, search, above, flash, pointing } = screen;
    let root =
        div().relative().size_full().overflow_hidden().when(pointing, |root| root.cursor_pointer());
    if let Some(notice) = notice {
        return root.child(render_notice(notice));
    }
    let runs = rows.iter().enumerate().flat_map(|(row, line)| render_row(row, line, cell));
    let cursor = cursor.map(|cursor| render_cursor(cursor, &rows, cell));
    let font = div().mono().text_size(cell.font_size).line_height(cell.height);
    let cells = font.absolute().inset_0().text_color(rgb(color::fg_body())).children(runs);
    let marks = render_marks(&rows, cell, inset.x, tip, cx);
    let padded = div().absolute().top(inset.y).bottom(inset.y).left(inset.x).right(inset.x);
    let fade = f32::from(inset.y + cell.height);
    let flashed = div()
        .absolute()
        .inset_0()
        .rounded_fill(10.0, glass::ink(0x0f))
        .bordered(color::warning() << 8 | 0x66);
    root.child(padded.child(cells.children(cursor)).children(marks))
        .when(above, |root| root.child(edge_fade(Edge::Top, fade, under)))
        .children(banner.map(render_banner))
        .children(search.map(render_search))
        .when(flash, |root| root.child(flashed))
}

fn render_row(row: usize, line: &ScreenRow, cell: Cell) -> Vec<Div> {
    let top = cell.height * row as f32;
    let mut column = 0;
    let runs = line.runs.iter().filter_map(|run| {
        let left = cell.width * column as f32;
        column += run.cells;
        let plain = run.face.background.is_none() && run.face.highlight.is_none();
        (!plain || !run.text.trim().is_empty()).then(|| render_run(run, left, top, cell))
    });
    runs.collect()
}

fn render_run(run: &CellRun, left: Pixels, top: Pixels, cell: Cell) -> Div {
    let (text, background) = colors(&run.face);
    let face = run.face;
    let (shown, pieces) = cell_glyphs::split(&run.text, run.cells, cell.width, cell.height)
        .unwrap_or_else(|| (run.text.clone(), Vec::new()));
    let drawn = !pieces.is_empty();
    let paint = move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut _| {
        paint_pieces(&pieces, bounds.origin, text, window)
    };
    let placed =
        div().absolute().left(left).top(top).w(cell.width * run.cells as f32).h(cell.height);
    placed
        .whitespace_nowrap()
        .text_color(text)
        .when_some(background, |run, background| run.bg(background))
        .when(face.bold, |run| run.font_weight(FontWeight::BOLD))
        .when(face.italic, |run| run.italic())
        .when(face.underline, |run| run.text_decoration_1().text_decoration_color(text))
        .when(face.strikeout, |run| run.line_through())
        .child(shown)
        .when(drawn, |run| {
            // Without insets it would sit below the text, at its place in the flow.
            run.child(canvas(|_, _, _| {}, paint).absolute().inset_0())
        })
}

fn paint_pieces(pieces: &[Piece], origin: Point<Pixels>, color: Hsla, window: &mut Window) {
    let at = |bounds: Bounds<Pixels>| Bounds::new(origin + bounds.origin, bounds.size);
    for &piece in pieces {
        let (mut path, points, count) = match piece {
            Piece::Fill { bounds, alpha } => {
                window.paint_quad(fill(at(bounds), Hsla { a: color.a * alpha, ..color }));
                continue;
            }
            Piece::Dot(bounds) => {
                window.paint_quad(fill(at(bounds), color).corner_radii(bounds.size.width * 0.5));
                continue;
            }
            Piece::Arc { bounds, corner, thickness } => {
                let radius = bounds.size.width.min(bounds.size.height);
                let mut radii = Corners::all(px(0.0));
                let mut edges = Edges::all(px(0.0));
                let (round, first, second) = match corner {
                    Corner::TopLeft => (&mut radii.top_left, &mut edges.top, &mut edges.left),
                    Corner::TopRight => (&mut radii.top_right, &mut edges.top, &mut edges.right),
                    Corner::BottomRight => {
                        (&mut radii.bottom_right, &mut edges.bottom, &mut edges.right)
                    }
                    Corner::BottomLeft => {
                        (&mut radii.bottom_left, &mut edges.bottom, &mut edges.left)
                    }
                };
                (*round, *first, *second) = (radius, thickness, thickness);
                let solid = BorderStyle::Solid;
                window.paint_quad(quad(
                    at(bounds),
                    radii,
                    transparent_black(),
                    edges,
                    color,
                    solid,
                ));
                continue;
            }
            Piece::Line { from, to, thickness } => {
                (PathBuilder::stroke(thickness), [from, to, to], 2)
            }
            Piece::Triangle(points) => (PathBuilder::fill(), points, 3),
        };
        // A line's two points stay open; a triangle's three close.
        path.add_polygon(&points.map(|point| origin + point)[..count], count == 3);
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    }
}

/// The text's color and the background's, the highlight over the cell's own.
fn colors(face: &CellFace) -> (Hsla, Option<Hsla>) {
    let foreground = face.foreground.map_or(color::fg_body(), console_color);
    let mut text = hsla(foreground);
    if face.dim {
        text.a = 0.6;
    }
    let background = face.background.map(|background| hsla(console_color(background)));
    let warning = |alpha| Some(rgba(color::warning() << 8 | alpha).into());
    match face.highlight {
        None => (text, background),
        Some(CellHighlight::Selection) => (text, Some(rgba(glass::selection()).into())),
        Some(CellHighlight::SearchMatch) => (hsla(color::fg_on_warning()), warning(0x3d)),
        Some(CellHighlight::SearchCurrent) => (hsla(color::bg_main()), warning(0xff)),
        Some(CellHighlight::Link) => (hsla(color::link()), background),
    }
}

fn render_cursor(cursor: ScreenCursor, rows: &[ScreenRow], cell: Cell) -> Div {
    let left = cell.width * cursor.column as f32;
    let height = cursor_height(cell.height, cell.glyph);
    let top = cell.height * cursor.row as f32 + (cell.height - height) / 2.0;
    let placed = |element: Div, top: Pixels, width: Pixels, height: Pixels| {
        element.absolute().left(left).top(top).w(width).h(height)
    };
    let bar = |top, width, height| placed(div(), top, width, height).bg(rgb(color::fg_main()));
    match cursor.shape {
        CursorShape::Block => placed(block_cursor(), top, cell.width, height)
            .flex()
            .items_center()
            .child(character_at(rows, cursor).unwrap_or_default()),
        CursorShape::Bar => bar(top, px(2.0), height),
        CursorShape::Underline => bar(top + height - px(2.0), cell.width, px(2.0)),
        CursorShape::Hollow => placed(div(), top, cell.width, height)
            .rounded(px(cursor::radius()))
            .bordered(color::fg_main() << 8 | 0x99),
    }
}

/// The character in the cell under a cursor, drawn inverted over it.
fn character_at(rows: &[ScreenRow], cursor: ScreenCursor) -> Option<String> {
    let mut column = 0;
    for run in &rows.get(cursor.row)?.runs {
        if cursor.column < column + run.cells {
            let index =
                if run.cells == run.text.chars().count() { cursor.column - column } else { 0 };
            return run.text.chars().nth(index).map(String::from);
        }
        column += run.cells;
    }
    None
}

/// Over the screen's left padding, an edge down the rows of each command that failed or
/// runs, and beside a failed one's prompt its exit code. Successes are left unmarked, as
/// most commands are, and nothing takes columns from the rows, so marks coming and going
/// never resize the screen.
fn render_marks(
    rows: &[ScreenRow],
    cell: Cell,
    padding: Pixels,
    tip: Option<((usize, bool), Look)>,
    cx: &mut Context<EditorView>,
) -> Vec<Div> {
    let mut marks = Vec::new();
    for (row, line) in rows.iter().enumerate() {
        let Some(mark) = line.mark.as_ref().filter(|mark| mark.exit != Some(0)) else {
            continue;
        };
        let end = rows[row + 1..]
            .iter()
            .position(|below| below.prompt)
            .map_or(rows.len(), |offset| row + 1 + offset);
        let plate = mark
            .exit
            .map(|code| format!("✗ {code}"))
            .filter(|plate| free_cells(line) > plate.chars().count() + 2);
        marks.push(render_mark(row, end, mark, plate, cell, padding, cx));
    }
    // Last, so that the marks below do not cover it.
    let tip = tip.and_then(|((row, on_plate), look)| {
        let mark = rows.get(row)?.mark.as_ref()?;
        Some(render_tip(row, mark, on_plate, look, cell))
    });
    marks.extend(tip);
    marks
}

/// The blank cells at the end of `row`, where a plate covers nothing.
fn free_cells(row: &ScreenRow) -> usize {
    let mut column = 0;
    let mut used = 0;
    for run in &row.runs {
        let trimmed = run.text.trim_end_matches(' ');
        if !trimmed.is_empty() {
            used = column + run.cells - (run.text.len() - trimmed.len());
        }
        column += run.cells;
    }
    column - used
}

fn render_mark(
    row: usize,
    end: usize,
    mark: &RowMark,
    plate: Option<String>,
    cell: Cell,
    padding: Pixels,
    cx: &mut Context<EditorView>,
) -> Div {
    let top = cell.height * row as f32;
    let tone = if mark.exit.is_some() { color::error() << 8 | 0xcc } else { glass::ink(0x4d) };
    let hover = move |on_plate: bool| {
        move |view: &mut EditorView, hovered: &bool, _: &mut Window, cx: &mut Context<_>| {
            view.hover_command_mark(hovered.then_some((row, on_plate)), cx);
        }
    };
    // As wide as the padding, so that the thin edge is easy to point at; with too little,
    // over the first column.
    let reach = padding.max(px(EDGE_REACH));
    let line = div().w(px(EDGE_WIDTH)).h_full().rounded_fill(1.5, tone);
    let edge = div()
        .id(("command-mark", row))
        .absolute()
        .left(-padding)
        .top(top + px(3.0))
        .w(reach)
        .h(cell.height * (end - row) as f32 - px(6.0))
        .flex()
        .justify_center()
        .child(line)
        .on_hover(cx.listener(hover(false)));
    let plate = plate.map(|plate| {
        let plate = div()
            .id(("command-plate", row))
            .row(0.0)
            .bar(PLATE_HEIGHT, 6.0)
            .rounded_fill(5.0, color::error() << 8 | 0x2e)
            .mono()
            .font_weight(FontWeight::BOLD)
            .text_size(cell.font_size - px(2.0))
            .text_color(rgb(color::error()))
            .child(plate)
            .on_hover(cx.listener(hover(true)));
        beside_row(top + (cell.height - px(PLATE_HEIGHT)) / 2.0, true).child(plate)
    });
    div().child(edge).children(plate)
}

/// A row across the screen at `top`, its content at its end, or its start.
fn beside_row(top: Pixels, at_end: bool) -> Div {
    div().across().top(top).flex().when(at_end, |row| row.justify_end())
}

/// How the command on `row` went, beside its plate or its edge, whichever the mouse is
/// over, and below the row where there is no room above it, coming out of the row as
/// `look` says.
fn render_tip(row: usize, mark: &RowMark, on_plate: bool, look: Look, cell: Cell) -> Div {
    let top = cell.height * row as f32;
    let tip_top = if top >= px(TIP_HEIGHT + 4.0) {
        top - px(TIP_HEIGHT + 4.0) + px(look.offset.abs())
    } else {
        top + cell.height + px(4.0) - px(look.offset.abs())
    };
    let tip = glass_surface(9.0)
        .row(8.0)
        .bar(TIP_HEIGHT, 10.0)
        .whitespace_nowrap()
        .mono_status(color::fg_body())
        .child(mark.detail.clone());
    beside_row(tip_top, on_plate).opacity(look.opacity).child(tip)
}

/// The shell starting, or why it could not, in place of the rows.
fn render_notice(notice: ScreenNotice) -> Div {
    let accent = tone_color(notice.tone, color::fg_dim());
    let danger = notice.tone == Tone::Danger;
    let title_color = if danger { accent } else { color::fg_main() };
    let title = div()
        .font_weight(FontWeight::SEMIBOLD)
        .text(font::ui_size() + 2.0, title_color)
        .child(notice.title);
    let detail = div().mono_status(color::fg_dim()).line_height(px(18.0)).child(notice.detail);
    let card = div()
        .column()
        .gap(px(8.0))
        .max_w(px(560.0))
        .p(px(18.0))
        .rounded(px(12.0))
        .when(danger, |card| {
            card.bg(rgba(color::error() << 8 | 0x14)).bordered(color::error() << 8 | 0x59)
        })
        .ui()
        .child(title)
        .child(detail);
    div().size_full().flex().items_center().justify_center().child(card)
}

/// That the program ended, over the bottom of the rows.
fn render_banner(banner: ScreenBanner) -> Div {
    let tone = tone_color(banner.tone, color::fg_dim());
    let keys = banner
        .keys
        .iter()
        .map(|&(keys, label)| key_hint(keys, label, 5.0).text_color(rgb(color::fg_dim())));
    let surface = glass_surface(10.0)
        .row(10.0)
        .bar(34.0, 12.0)
        .ui()
        .status_text(color::fg_body())
        .child(div().size(px(8.0)).rounded_full().bg(rgb(tone)))
        .child(banner.text)
        .children(keys);
    bottom_centered(8.0, surface)
}

/// `I-search:` and what is typed, with the match shown and how many there are.
fn render_search(search: ScreenSearch) -> Div {
    let found = match search.found {
        Some((current, total)) => format!("{current} of {total}"),
        None if search.input.is_empty() => String::new(),
        None => "no match".to_owned(),
    };
    let caret = inline_cursor().ml(px(1.0)).child(" ");
    let input =
        div().row(0.0).mono().text_color(rgb(color::fg_main())).child(search.input).child(caret);
    let hints = search
        .hints
        .iter()
        .map(|&(keys, label)| key_hint(keys, label, 5.0).status_text(color::fg_dim()));
    let bar = glass_surface(14.0)
        .row(10.0)
        .w(px(SEARCH_BAR_WIDTH))
        .h(px(SEARCH_BAR_HEIGHT))
        .pl(px(14.0))
        .pr(px(12.0))
        .ui()
        .text_size(px(font::ui_size()))
        .child(div().text_color(rgb(color::fg_dim())).child("I-search:"))
        .child(input)
        .child(div().flex_1())
        .child(div().mono_status(color::warning()).child(found))
        .children(hints);
    bottom_centered(10.0, bar)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(runs: &[&str]) -> ScreenRow {
        let run = |text: &&str| CellRun {
            text: text.to_string(),
            cells: text.chars().count(),
            face: CellFace::default(),
        };
        ScreenRow { runs: runs.iter().map(run).collect(), ..ScreenRow::default() }
    }

    #[test]
    fn free_cells_count_the_blanks_after_the_last_text() {
        assert_eq!(free_cells(&row(&["$ ls", "      "])), 6);
        assert_eq!(free_cells(&row(&["$ ls   ", "  "])), 5);
        // A right prompt leaves no room for a plate.
        assert_eq!(free_cells(&row(&["$ ls   ", "10:41"])), 0);
        assert_eq!(free_cells(&row(&["        "])), 8);
    }
}
