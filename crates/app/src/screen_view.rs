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
use crate::widgets::{Edge, block_cursor, edge_fade, glass_surface, hsla, key_caps, tone_color};

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
    let Screen {
        rows,
        cursor,
        banner,
        notice,
        mode: _,
        search,
        above,
        flash,
        pointing,
    } = screen;
    let root = div()
        .relative()
        .size_full()
        .overflow_hidden()
        .when(pointing, |root| root.cursor_pointer());
    if let Some(notice) = notice {
        return root.child(render_notice(notice));
    }
    let padded = div()
        .absolute()
        .top(inset.y)
        .bottom(inset.y)
        .left(inset.x)
        .right(inset.x);
    let cells = div()
        .absolute()
        .inset_0()
        .font_family(font::mono())
        .text_size(px(font::code().size))
        .line_height(cell.height)
        .text_color(rgb(color::fg_body()))
        .children(
            rows.iter()
                .enumerate()
                .flat_map(|(row, line)| render_row(row, line, cell)),
        )
        .children(cursor.map(|cursor| render_cursor(cursor, &rows, cell)));
    let marks = render_marks(&rows, cell, inset.x, tip, cx);
    let padded = padded.child(cells).children(marks);
    let fade = f32::from(inset.y + cell.height);
    root.child(padded)
        .when(above, |root| root.child(edge_fade(Edge::Top, fade, under)))
        .children(banner.map(render_banner))
        .children(search.map(render_search))
        .when(flash, |root| {
            root.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(10.0))
                    .bg(rgba(glass::ink(0x0f)))
                    .border_1()
                    .border_color(rgba(color::warning() << 8 | 0x66)),
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
    let (shown, pieces) = cell_glyphs::split(&run.text, run.cells, cell.width, cell.height)
        .unwrap_or_else(|| (run.text.clone(), Vec::new()));
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
        .child(shown)
        .when(!pieces.is_empty(), |run| {
            run.child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| paint_pieces(&pieces, bounds.origin, text, window),
                )
                // Without insets it would sit below the text, at its place in the flow.
                .absolute()
                .inset_0(),
            )
        })
}

fn paint_pieces(pieces: &[Piece], origin: Point<Pixels>, color: Hsla, window: &mut Window) {
    let at = |bounds: Bounds<Pixels>| Bounds::new(origin + bounds.origin, bounds.size);
    for &piece in pieces {
        match piece {
            Piece::Fill { bounds, alpha } => window.paint_quad(fill(
                at(bounds),
                Hsla {
                    a: color.a * alpha,
                    ..color
                },
            )),
            Piece::Dot(bounds) => {
                window.paint_quad(fill(at(bounds), color).corner_radii(bounds.size.width * 0.5))
            }
            Piece::Arc {
                bounds,
                corner,
                thickness,
            } => {
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
                window.paint_quad(quad(
                    at(bounds),
                    radii,
                    transparent_black(),
                    edges,
                    color,
                    BorderStyle::Solid,
                ));
            }
            Piece::Line {
                from,
                to,
                thickness,
            } => {
                let mut path = PathBuilder::stroke(thickness);
                path.move_to(origin + from);
                path.line_to(origin + to);
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            }
            Piece::Triangle(points) => {
                let mut path = PathBuilder::fill();
                path.add_polygon(&points.map(|point| origin + point), true);
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            }
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
    let background = face
        .background
        .map(|background| hsla(console_color(background)));
    match face.highlight {
        None => (text, background),
        Some(CellHighlight::Selection) => (text, Some(rgba(glass::selection()).into())),
        Some(CellHighlight::SearchMatch) => (
            hsla(color::fg_on_warning()),
            Some(rgba(color::warning() << 8 | 0x3d).into()),
        ),
        Some(CellHighlight::SearchCurrent) => {
            (hsla(color::bg_main()), Some(hsla(color::warning())))
        }
        Some(CellHighlight::Link) => (hsla(color::link()), background),
    }
}

fn render_cursor(cursor: ScreenCursor, rows: &[ScreenRow], cell: Cell) -> Div {
    let left = cell.width * cursor.column as f32;
    let top = cell.height * cursor.row as f32;
    let glyph_top = top + (cell.height - cell.glyph) / 2.0;
    let quad = div().absolute().left(left);
    match cursor.shape {
        CursorShape::Block => block_cursor()
            .absolute()
            .left(left)
            .top(glyph_top)
            .w(cell.width)
            .h(cell.glyph)
            .flex()
            .items_center()
            .child(character_at(rows, cursor).unwrap_or_default()),
        CursorShape::Bar => quad
            .top(glyph_top)
            .w(px(2.0))
            .h(cell.glyph)
            .bg(rgb(color::fg_main())),
        CursorShape::Underline => quad
            .top(glyph_top + cell.glyph - px(2.0))
            .w(cell.width)
            .h(px(2.0))
            .bg(rgb(color::fg_main())),
        CursorShape::Hollow => quad
            .top(glyph_top)
            .w(cell.width)
            .h(cell.glyph)
            .rounded(px(cursor::radius()))
            .border_1()
            .border_color(rgba(color::fg_main() << 8 | 0x99)),
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
    let failed = mark.exit.is_some();
    let tone = if failed {
        rgba(color::error() << 8 | 0xcc)
    } else {
        rgba(glass::ink(0x4d))
    };
    let hover = move |on_plate: bool| {
        move |view: &mut EditorView,
              hovered: &bool,
              _: &mut Window,
              cx: &mut Context<EditorView>| {
            view.hover_command_mark(hovered.then_some((row, on_plate)), cx);
        }
    };
    // As wide as the padding, so that the thin edge is easy to point at; with too little,
    // over the first column.
    let reach = padding.max(px(EDGE_REACH));
    let edge = div()
        .id(("command-mark", row))
        .absolute()
        .left(-padding)
        .top(top + px(3.0))
        .w(reach)
        .h(cell.height * (end - row) as f32 - px(6.0))
        .flex()
        .justify_center()
        .child(div().w(px(EDGE_WIDTH)).h_full().rounded(px(1.5)).bg(tone))
        .on_hover(cx.listener(hover(false)));
    let plate = plate.map(|plate| {
        div()
            .absolute()
            .left_0()
            .right_0()
            .top(top + (cell.height - px(PLATE_HEIGHT)) / 2.0)
            .flex()
            .justify_end()
            .child(
                div()
                    .id(("command-plate", row))
                    .h(px(PLATE_HEIGHT))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .rounded(px(5.0))
                    .bg(rgba(color::error() << 8 | 0x2e))
                    .font_family(font::mono())
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(font::code().size - 2.0))
                    .text_color(rgb(color::error()))
                    .child(plate)
                    .on_hover(cx.listener(hover(true))),
            )
    });
    div().child(edge).children(plate)
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
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(TIP_HEIGHT))
        .px(px(10.0))
        .whitespace_nowrap()
        .font_family(font::mono())
        .text_size(px(font::status_size()))
        .text_color(rgb(color::fg_body()))
        .child(mark.detail.clone());
    div()
        .absolute()
        .left_0()
        .right_0()
        .top(tip_top)
        .opacity(look.opacity)
        .flex()
        .when(on_plate, |row| row.justify_end())
        .child(tip)
}

/// The shell starting, or why it could not, in place of the rows.
fn render_notice(notice: ScreenNotice) -> Div {
    let accent = tone_color(notice.tone, color::fg_dim());
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
                    card.bg(rgba(color::error() << 8 | 0x14))
                        .border_1()
                        .border_color(rgba(color::error() << 8 | 0x59))
                })
                .font_family(font::ui())
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(font::ui_size() + 2.0))
                        .text_color(rgb(if danger { accent } else { color::fg_main() }))
                        .child(notice.title),
                )
                .child(
                    div()
                        .font_family(font::mono())
                        .text_size(px(font::status_size()))
                        .line_height(px(18.0))
                        .text_color(rgb(color::fg_dim()))
                        .child(notice.detail),
                ),
        )
}

/// That the program ended, over the bottom of the rows.
fn render_banner(banner: ScreenBanner) -> Div {
    let tone = tone_color(banner.tone, color::fg_dim());
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
                .font_family(font::ui())
                .text_size(px(font::status_size()))
                .text_color(rgb(color::fg_body()))
                .child(div().size(px(8.0)).rounded_full().bg(rgb(tone)))
                .child(banner.text)
                .children(banner.keys.iter().map(|&(keys, label)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .text_color(rgb(color::fg_dim()))
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
                .font_family(font::ui())
                .text_size(px(font::ui_size()))
                .child(div().text_color(rgb(color::fg_dim())).child("I-search:"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .font_family(font::mono())
                        .text_color(rgb(color::fg_main()))
                        .child(search.input)
                        .child(
                            div()
                                .ml(px(1.0))
                                .w(px(8.0))
                                .h(px(17.0))
                                .rounded(px(1.0))
                                .bg(rgb(color::fg_main())),
                        ),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .font_family(font::mono())
                        .text_size(px(font::status_size()))
                        .text_color(rgb(color::warning()))
                        .child(found),
                )
                .children(search.hints.iter().map(|&(keys, label)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .text_size(px(font::status_size()))
                        .text_color(rgb(color::fg_dim()))
                        .children(key_caps(keys))
                        .child(label)
                })),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(runs: &[&str]) -> ScreenRow {
        ScreenRow {
            runs: runs
                .iter()
                .map(|text| CellRun {
                    text: (*text).to_owned(),
                    cells: text.chars().count(),
                    face: CellFace::default(),
                })
                .collect(),
            ..ScreenRow::default()
        }
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
