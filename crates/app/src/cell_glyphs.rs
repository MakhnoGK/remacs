//! Terminal graphics — box drawing, blocks, braille and the like — drawn as shapes filling
//! their cell rather than from the font, so they meet across rows however tall the line
//! height makes the cell.

use std::ops::Range;

use gpui::{Bounds, Corner, Pixels, Point, point, px, size};

/// One shape of a character, placed within its run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Piece {
    /// A rectangle in the text's color, `alpha` of it for the shades.
    Fill {
        bounds: Bounds<Pixels>,
        alpha: f32,
    },
    /// A round dot filling `bounds`.
    Dot(Bounds<Pixels>),
    /// A rounded corner: the two sides of `bounds` that meet at `corner`, `thickness` wide.
    Arc {
        bounds: Bounds<Pixels>,
        corner: Corner,
        thickness: Pixels,
    },
    /// A straight stroke at any angle.
    Line {
        from: Point<Pixels>,
        to: Point<Pixels>,
        thickness: Pixels,
    },
    Triangle([Point<Pixels>; 3]),
}

/// How much of its share of the cell a dash of a dashed line takes.
const DASH: f32 = 0.6;

const LIGHT: u16 = 1;
const HEAVY: u16 = 2;
const DOUBLE: u16 = 3;

/// U+2500 to U+256C, each arm's weight from the cell's center, a nibble each: up, right,
/// down, left; 1 light, 2 heavy, 3 double. Zero marks the dashed lines.
#[rustfmt::skip]
const LINES: [u16; 109] = [
    // ─━│┃┄┅┆┇┈┉┊┋
    0x0101, 0x0202, 0x1010, 0x2020, 0, 0, 0, 0, 0, 0, 0, 0,
    // ┌┍┎┏┐┑┒┓
    0x0110, 0x0210, 0x0120, 0x0220, 0x0011, 0x0012, 0x0021, 0x0022,
    // └┕┖┗┘┙┚┛
    0x1100, 0x1200, 0x2100, 0x2200, 0x1001, 0x1002, 0x2001, 0x2002,
    // ├┝┞┟┠┡┢┣
    0x1110, 0x1210, 0x2110, 0x1120, 0x2120, 0x2210, 0x1220, 0x2220,
    // ┤┥┦┧┨┩┪┫
    0x1011, 0x1012, 0x2011, 0x1021, 0x2021, 0x2012, 0x1022, 0x2022,
    // ┬┭┮┯┰┱┲┳
    0x0111, 0x0112, 0x0211, 0x0212, 0x0121, 0x0122, 0x0221, 0x0222,
    // ┴┵┶┷┸┹┺┻
    0x1101, 0x1102, 0x1201, 0x1202, 0x2101, 0x2102, 0x2201, 0x2202,
    // ┼┽┾┿╀╁╂╃
    0x1111, 0x1112, 0x1211, 0x1212, 0x2111, 0x1121, 0x2121, 0x2112,
    // ╄╅╆╇╈╉╊╋╌╍╎╏
    0x2211, 0x1122, 0x1221, 0x2212, 0x1222, 0x2122, 0x2221, 0x2222, 0, 0, 0, 0,
    // ═║╒╓╔╕╖╗
    0x0303, 0x3030, 0x0310, 0x0130, 0x0330, 0x0013, 0x0031, 0x0033,
    // ╘╙╚╛╜╝╞╟
    0x1300, 0x3100, 0x3300, 0x1003, 0x3001, 0x3003, 0x1310, 0x3130,
    // ╠╡╢╣╤╥╦╧
    0x3330, 0x1013, 0x3031, 0x3033, 0x0313, 0x0131, 0x0333, 0x1303,
    // ╨╩╪╫╬
    0x3101, 0x3303, 0x1313, 0x3131, 0x3333,
];
/// ╴╵╶╷╸╹╺╻╼╽╾╿
#[rustfmt::skip]
const STUBS: [u16; 12] = [
    0x0001, 0x1000, 0x0100, 0x0010, 0x0002, 0x2000, 0x0200, 0x0020, 0x0201, 0x1020, 0x0102, 0x2010,
];
/// ▖▗▘▙▚▛▜▝▞▟ as their quarters: 1 upper left, 2 upper right, 4 lower left, 8 lower right.
const QUADRANTS: [u8; 10] = [4, 8, 1, 13, 9, 7, 11, 2, 6, 14];

/// `text` with its lines and blocks left blank, and the pieces drawn in their place in
/// cells `width` × `height`; none when it has none or its characters are not one cell each.
pub fn split(
    text: &str,
    cells: usize,
    width: Pixels,
    height: Pixels,
) -> Option<(String, Vec<Piece>)> {
    if !text.chars().any(|character| drawn(character as u32)) || text.chars().count() != cells {
        return None;
    }
    let mut shown = String::with_capacity(text.len());
    let mut all = Vec::with_capacity(cells);
    // One character's pieces at a time, reused so a frame does not allocate per character.
    let mut own = Vec::new();
    for (column, character) in text.chars().enumerate() {
        own.clear();
        if !pieces(character as u32, width, height, &mut own) {
            shown.push(character);
            continue;
        }
        shown.push(' ');
        let left = width * column as f32;
        for &piece in &own {
            push(&mut all, shifted(piece, left));
        }
    }
    Some((shown, all))
}

/// Box drawing, block elements, braille, the sextants and eighth lines of the legacy
/// computing symbols, and powerline's triangles: the ranges `pieces` draws.
fn drawn(code: u32) -> bool {
    matches!(
        code,
        0x2500..=0x259F | 0x2800..=0x28FF | 0x1FB00..=0x1FB3B | 0x1FB70..=0x1FB7B | 0xE0B0..=0xE0B3
    )
}

/// Adds the pieces of the character `code` to `out`; false when the font draws it.
fn pieces(code: u32, width: Pixels, height: Pixels, out: &mut Vec<Piece>) -> bool {
    match code {
        0x2500..=0x256C => match LINES[(code - 0x2500) as usize] {
            0 => dashed(code, width, height, out),
            arms => lines(arms, width, height, out),
        },
        0x256D..=0x2570 => out.push(arc(code, width, height)),
        0x2571..=0x2573 => diagonals(code, width, height, out),
        0x2574..=0x257F => lines(STUBS[(code - 0x2574) as usize], width, height, out),
        0x2580..=0x259F | 0x1FB70..=0x1FB7B => block(code, width, height, out),
        0x2800..=0x28FF => braille(code, width, height, out),
        0x1FB00..=0x1FB3B => sextant(code, width, height, out),
        0xE0B0..=0xE0B3 => powerline(code, width, height, out),
        _ => return false,
    }
    true
}

fn light(width: Pixels) -> Pixels {
    (width * 0.125).round().max(px(1.0))
}

/// Where a line `thickness` wide starts to sit centered across `extent`.
fn center(extent: Pixels, thickness: Pixels) -> Pixels {
    ((extent - thickness) * 0.5).round()
}

fn fill(x: Pixels, y: Pixels, width: Pixels, height: Pixels) -> Piece {
    Piece::Fill { bounds: Bounds::new(point(x, y), size(width, height)), alpha: 1.0 }
}

const UP: u16 = 0;
const RIGHT: u16 = 1;
const DOWN: u16 = 2;
const LEFT: u16 = 3;

/// The strokes of a line of `weight` across `extent`, as where each starts and how thick it
/// is, in order; the first `count` of the two.
fn strokes(weight: u16, extent: Pixels, light: Pixels) -> ([(Pixels, Pixels); 2], usize) {
    match weight {
        LIGHT => ([(center(extent, light), light); 2], 1),
        HEAVY => ([(center(extent, light * 2.0), light * 2.0); 2], 1),
        DOUBLE => {
            let middle = center(extent, light);
            ([(middle - light, light), (middle + light, light)], 2)
        }
        _ => ([(px(0.0), px(0.0)); 2], 0),
    }
}

/// Each arm's strokes run from the cell's edge to the strokes of the arms beside it: a
/// stroke facing an arm stops at that arm's nearer stroke, an outer one wraps around to the
/// other arm's farther stroke, so double lines turn and cross as two lines do.
fn lines(arms: u16, width: Pixels, height: Pixels, out: &mut Vec<Piece>) {
    let light = light(width);
    let weight = |arm: u16| (arms >> (12 - 4 * arm)) & 0xF;
    let first = out.len();
    let mut verticals = first;
    // Up and down, then left and right, so the halves of a straight line join into one.
    for arm in [UP, DOWN, LEFT, RIGHT] {
        let own = weight(arm);
        if own == 0 {
            continue;
        }
        let vertical = arm == UP || arm == DOWN;
        let (along, across) = if vertical { (height, width) } else { (width, height) };
        // `before` is beside the arm's first stroke, `after` beside its last.
        let (before, after) = if vertical { (LEFT, RIGHT) } else { (UP, DOWN) };
        let from_edge_start = arm == UP || arm == LEFT;
        let span = |beside: u16, near: bool| {
            let (spans, count) = strokes(weight(beside), along, light);
            let first = if near == from_edge_start { 0 } else { count - 1 };
            spans[first]
        };
        let (own_strokes, count) = strokes(own, across, light);
        for (index, &(offset, thickness)) in own_strokes[..count].iter().enumerate() {
            let (side, other) = if index == 0 { (before, after) } else { (after, before) };
            // A single line passes straight through two arms beside it, as ┼ and ╪ do.
            let reached = match (own == DOUBLE, weight(side) != 0, weight(other) != 0) {
                (true, true, _) => Some(span(side, true)),
                (true, false, true) | (false, false, true) => Some(span(other, false)),
                (false, true, false) => Some(span(side, false)),
                _ => None,
            };
            let middle = center(along, light);
            let (start, end) = match (from_edge_start, reached) {
                (true, Some((start, thick))) => (px(0.0), start + thick),
                (true, None) => (px(0.0), middle + light),
                (false, Some((start, _))) => (start, along),
                (false, None) => (middle, along),
            };
            if vertical {
                push(out, fill(offset, start, thickness, end - start));
                verticals = out.len();
            } else {
                let stroke = Bounds::new(point(start, offset), size(end - start, thickness));
                push_uncovered(out, first..verticals, stroke);
            }
        }
    }
}

/// Adds the parts of the horizontal `stroke` that the vertical strokes at `verticals` in
/// `out` do not cross, so where lines meet is painted once and translucent (dim) text
/// shows no brighter joints.
fn push_uncovered(out: &mut Vec<Piece>, verticals: Range<usize>, stroke: Bounds<Pixels>) {
    let mut cuts = [(px(0.0), px(0.0)); 4];
    let mut count = 0;
    for piece in &out[verticals] {
        if let Piece::Fill { bounds, .. } = piece
            && bounds.top() <= stroke.top()
            && bounds.bottom() >= stroke.bottom()
        {
            cuts[count] = (bounds.left(), bounds.right());
            count += 1;
        }
    }
    let cuts = &mut cuts[..count];
    cuts.sort_by(|a, b| f32::from(a.0).total_cmp(&f32::from(b.0)));
    let (top, thickness) = (stroke.top(), stroke.size.height);
    let mut start = stroke.left();
    for &(cut_left, cut_right) in cuts.iter() {
        let end = cut_left.min(stroke.right());
        if end > start {
            push(out, fill(start, top, end - start, thickness));
        }
        start = start.max(cut_right);
    }
    if stroke.right() > start {
        push(out, fill(start, top, stroke.right() - start, thickness));
    }
}

fn arc(code: u32, width: Pixels, height: Pixels) -> Piece {
    let thickness = light(width);
    let x = center(width, thickness);
    let y = center(height, thickness);
    let (right, bottom) = (x + thickness, y + thickness);
    let (origin, extent, corner) = match code {
        0x256D => (point(x, y), size(width - x, height - y), Corner::TopLeft),
        0x256E => (point(px(0.0), y), size(right, height - y), Corner::TopRight),
        0x256F => (point(px(0.0), px(0.0)), size(right, bottom), Corner::BottomRight),
        _ => (point(x, px(0.0)), size(width - x, bottom), Corner::BottomLeft),
    };
    Piece::Arc { bounds: Bounds::new(origin, extent), corner, thickness }
}

fn block(code: u32, width: Pixels, height: Pixels, out: &mut Vec<Piece>) {
    let part = |x: f32, y: f32, w: f32, h: f32| fill(width * x, height * y, width * w, height * h);
    let eighths = |count: u32| count as f32 / 8.0;
    let piece = match code {
        0x2580 => part(0.0, 0.0, 1.0, 0.5),
        0x2581..=0x2588 => {
            let h = eighths(code - 0x2580);
            part(0.0, 1.0 - h, 1.0, h)
        }
        0x2589..=0x258F => part(0.0, 0.0, eighths(0x2590 - code), 1.0),
        0x2590 => part(0.5, 0.0, 0.5, 1.0),
        0x2591..=0x2593 => Piece::Fill {
            bounds: Bounds::new(point(px(0.0), px(0.0)), size(width, height)),
            alpha: (code - 0x2590) as f32 / 4.0,
        },
        0x2594 => part(0.0, 0.0, 1.0, 0.125),
        0x2595 => part(0.875, 0.0, 0.125, 1.0),
        0x1FB70..=0x1FB75 => part(eighths(code - 0x1FB6F), 0.0, 0.125, 1.0),
        0x1FB76..=0x1FB7B => part(0.0, eighths(code - 0x1FB75), 1.0, 0.125),
        _ => {
            let quarters = QUADRANTS[(code - 0x2596) as usize];
            for (bit, x, y) in [(1, 0.0, 0.0), (2, 0.5, 0.0), (4, 0.0, 0.5), (8, 0.5, 0.5)] {
                if quarters & bit != 0 {
                    push(out, part(x, y, 0.5, 0.5));
                }
            }
            return;
        }
    };
    out.push(piece);
}

/// ┄┅┆┇┈┉┊┋ and ╌╍╎╏: every other one heavy, every other pair down, in threes, fours and twos.
fn dashed(code: u32, width: Pixels, height: Pixels, out: &mut Vec<Piece>) {
    let (index, count) = match code {
        0x2504..=0x250B => (code - 0x2504, 3 + (code - 0x2504) / 4),
        _ => (code - 0x254C, 2),
    };
    let thickness = light(width) * if index % 2 == 1 { 2.0 } else { 1.0 };
    let vertical = index / 2 % 2 == 1;
    let (along, across) = if vertical { (height, width) } else { (width, height) };
    let segment = along * (1.0 / count as f32);
    let dash = segment * DASH;
    let offset = center(across, thickness);
    out.extend((0..count).map(|index| {
        // Half a gap at each end, so dashes keep their spacing across cells.
        let start = segment * index as f32 + (segment - dash) * 0.5;
        if vertical {
            fill(offset, start, thickness, dash)
        } else {
            fill(start, offset, dash, thickness)
        }
    }));
}

/// ╱╲╳ from corner to corner.
// shortcut: ╳'s two strokes overlap at the center, so with dim text it shows a brighter
// spot there; draw it as one outline polygon if that is ever seen.
fn diagonals(code: u32, width: Pixels, height: Pixels, out: &mut Vec<Piece>) {
    let thickness = light(width);
    let rising = line(point(px(0.0), height), point(width, px(0.0)), thickness);
    let falling = line(point(px(0.0), px(0.0)), point(width, height), thickness);
    match code {
        0x2571 => out.push(rising),
        0x2572 => out.push(falling),
        _ => out.extend([rising, falling]),
    }
}

/// Eight dots spread over the whole cell, so a graph of them runs on across rows.
fn braille(code: u32, width: Pixels, height: Pixels, out: &mut Vec<Piece>) {
    // Dots 1 to 8 by their bit, as column and row: 1–3 and 7 down the left, 4–6 and 8 the right.
    const PLACES: [(f32, f32); 8] =
        [(0., 0.), (0., 1.), (0., 2.), (1., 0.), (1., 1.), (1., 2.), (0., 3.), (1., 3.)];
    let dots = code - 0x2800;
    let diameter = (width * 0.25).max(px(1.0));
    out.extend(PLACES.iter().enumerate().filter(|&(bit, _)| dots >> bit & 1 == 1).map(
        |(_, &(column, row))| {
            let x = width * (0.25 + 0.5 * column) - diameter * 0.5;
            let y = height * (0.125 + 0.25 * row) - diameter * 0.5;
            Piece::Dot(Bounds::new(point(x, y), size(diameter, diameter)))
        },
    ));
}

/// 🬀 to 🬻: the cell in two columns and three rows, every combination but the empty, full
/// and half ones, which have characters of their own.
fn sextant(code: u32, width: Pixels, height: Pixels, out: &mut Vec<Piece>) {
    const LEFT_HALF: u32 = 0b010101;
    const RIGHT_HALF: u32 = 0b101010;
    let mut sixths = code - 0x1FB00 + 1;
    sixths += u32::from(sixths >= LEFT_HALF);
    sixths += u32::from(sixths >= RIGHT_HALF);
    for bit in 0..6 {
        if sixths >> bit & 1 == 1 {
            let (column, row) = ((bit % 2) as f32, (bit / 2) as f32);
            let (x, y) = (width * (column * 0.5), height * (row / 3.0));
            push(out, fill(x, y, width * 0.5, height * (1.0 / 3.0)));
        }
    }
}

/// U+E0B0 to U+E0B3: a solid triangle pointing right, its chevron, and the two pointing
/// left, from the cell's top to its bottom.
// shortcut: a chevron's two strokes overlap at its tip, brighter with dim text; draw it as
// one outline polygon if that is ever seen.
fn powerline(code: u32, width: Pixels, height: Pixels, out: &mut Vec<Piece>) {
    let (base, tip) = if code < 0xE0B2 { (px(0.0), width) } else { (width, px(0.0)) };
    let (top, middle, bottom) =
        (point(base, px(0.0)), point(tip, height * 0.5), point(base, height));
    if code.is_multiple_of(2) {
        out.push(Piece::Triangle([top, middle, bottom]));
        return;
    }
    let thickness = light(width);
    out.extend([line(top, middle, thickness), line(middle, bottom, thickness)]);
}

fn line(from: Point<Pixels>, to: Point<Pixels>, thickness: Pixels) -> Piece {
    Piece::Line { from, to, thickness }
}

fn shifted(mut piece: Piece, left: Pixels) -> Piece {
    let by = |at: &mut Point<Pixels>| at.x += left;
    match &mut piece {
        Piece::Fill { bounds, .. } | Piece::Dot(bounds) | Piece::Arc { bounds, .. } => {
            by(&mut bounds.origin)
        }
        Piece::Line { from, to, .. } => [from, to].into_iter().for_each(by),
        Piece::Triangle(points) => points.iter_mut().for_each(by),
    }
    piece
}

/// Adds `piece`, joined into one of the last few when together they make one rectangle, so
/// a line across many cells is one quad and shows no seams; the few cover a cell's strokes.
fn push(pieces: &mut Vec<Piece>, piece: Piece) {
    const RECENT: usize = 4;
    if let Piece::Fill { bounds, alpha } = piece
        && let Some(last) = pieces.iter_mut().rev().take(RECENT).find_map(|recent| match recent {
            Piece::Fill { bounds: last, alpha: last_alpha }
                if *last_alpha == alpha && joins(last, &bounds) =>
            {
                Some(last)
            }
            _ => None,
        })
    {
        *last = last.union(&bounds);
        return;
    }
    pieces.push(piece);
}

fn joins(a: &Bounds<Pixels>, b: &Bounds<Pixels>) -> bool {
    // Columns are placed by multiplying, so the edges of neighbors may differ by a rounding.
    let slack = px(0.5);
    let meet =
        |a0: Pixels, a1: Pixels, b0: Pixels, b1: Pixels| a0 <= b1 + slack && b0 <= a1 + slack;
    (a.origin.y == b.origin.y
        && a.size.height == b.size.height
        && meet(a.left(), a.right(), b.left(), b.right()))
        || (a.origin.x == b.origin.x
            && a.size.width == b.size.width
            && meet(a.top(), a.bottom(), b.top(), b.bottom()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: Pixels = px(8.0);
    const HEIGHT: Pixels = px(24.0);

    fn bounds(piece: &Piece) -> Bounds<Pixels> {
        match piece {
            Piece::Fill { bounds, .. } | Piece::Dot(bounds) | Piece::Arc { bounds, .. } => *bounds,
            Piece::Line { from, to, .. } => around(&[*from, *to]),
            Piece::Triangle(points) => around(points),
        }
    }

    fn around(points: &[Point<Pixels>]) -> Bounds<Pixels> {
        let corner = |pick: fn(Pixels, Pixels) -> Pixels| {
            points.iter().copied().reduce(|a, b| point(pick(a.x, b.x), pick(a.y, b.y))).unwrap()
        };
        Bounds::from_corners(corner(Pixels::min), corner(Pixels::max))
    }

    /// The pieces of `text`, one cell per character.
    fn pieces_of(text: &str) -> Vec<Piece> {
        split(text, text.chars().count(), WIDTH, HEIGHT).unwrap().1
    }

    /// The bounds around all of `pieces`.
    fn union(pieces: &[Piece]) -> Bounds<Pixels> {
        pieces.iter().map(bounds).reduce(|a, b| a.union(&b)).unwrap()
    }

    #[test]
    fn where_lines_meet_is_painted_once() {
        for text in ["┌", "┼", "╋", "┿", "├", "┍", "╜", "╔", "╬", "╪", "╫", "╠"]
        {
            let pieces = pieces_of(text);
            for (index, a) in pieces.iter().map(bounds).enumerate() {
                for b in pieces[index + 1..].iter().map(bounds) {
                    let overlap_x = a.right().min(b.right()) - a.left().max(b.left());
                    let overlap_y = a.bottom().min(b.bottom()) - a.top().max(b.top());
                    assert!(
                        overlap_x <= px(0.0) || overlap_y <= px(0.0),
                        "{text}: {a:?} and {b:?} overlap"
                    );
                }
            }
        }
    }

    #[test]
    fn braille_dots_spread_over_the_whole_cell() {
        let pieces = pieces_of("⣿");
        assert_eq!(pieces.len(), 8);
        let all = union(&pieces);
        assert!(all.top() < HEIGHT * 0.125 && all.bottom() > HEIGHT * 0.875);
    }

    #[test]
    fn dashed_lines_have_their_count_of_dashes() {
        assert_eq!(pieces_of("┆").len(), 3);
        assert_eq!(pieces_of("┉").len(), 4);
        assert_eq!(pieces_of("╎").len(), 2);
    }

    #[test]
    fn sextants_skip_the_halves_that_have_their_own_characters() {
        // 🬀 is the upper left sixth alone.
        assert_eq!(pieces_of("🬀"), vec![fill(px(0.0), px(0.0), WIDTH * 0.5, HEIGHT * (1.0 / 3.0))]);
        // 🬻, the last, is all but the upper left: the right column and the lower left two.
        assert_eq!(pieces_of("🬻").len(), 2);
    }

    #[test]
    fn diagonals_and_powerline_triangles_span_the_cell() {
        for text in ["╱", "╳", "\u{E0B0}", "\u{E0B3}"] {
            assert_eq!(union(&pieces_of(text)).size, size(WIDTH, HEIGHT), "{text}");
        }
    }

    #[test]
    fn a_vertical_line_fills_the_cell_height() {
        let pieces = pieces_of("│");
        assert_eq!(pieces.len(), 1);
        let line = bounds(&pieces[0]);
        assert_eq!((line.top(), line.bottom()), (px(0.0), HEIGHT));
    }

    #[test]
    fn a_horizontal_line_across_cells_is_one_piece() {
        let (shown, pieces) = split("a───b", 5, WIDTH, HEIGHT).unwrap();
        assert_eq!(shown, "a   b");
        assert_eq!(pieces.len(), 1);
        let line = bounds(&pieces[0]);
        assert_eq!((line.left(), line.right()), (WIDTH, WIDTH * 4.0));
    }

    #[test]
    fn a_corner_reaches_the_bottom_and_right_edges() {
        let union = union(&pieces_of("┌"));
        assert_eq!((union.right(), union.bottom()), (WIDTH, HEIGHT));
    }

    #[test]
    fn a_double_line_across_cells_is_two_pieces() {
        let pieces = pieces_of("═══");
        assert_eq!(pieces.len(), 2);
        assert!(pieces.iter().all(|piece| bounds(piece).size.width == WIDTH * 3.0));
    }

    #[test]
    fn a_double_corner_wraps_its_outer_line_around_the_inner() {
        let pieces = pieces_of("╔");
        let outer = union(&pieces);
        assert_eq!((outer.right(), outer.bottom()), (WIDTH, HEIGHT));
        // The inner line's two strokes start past the outer line's vertical stroke.
        assert_eq!(pieces.len(), 4);
        let inner = pieces
            .iter()
            .filter(|piece| bounds(piece).left() > outer.left() + light(WIDTH))
            .count();
        assert_eq!(inner, 2);
    }

    #[test]
    fn a_single_line_crosses_a_double_one_unbroken() {
        assert!(pieces_of("╪").iter().any(|piece| bounds(piece).size.height == HEIGHT));
        assert!(pieces_of("╬").iter().all(|piece| {
            let piece = bounds(piece);
            piece.size.height < HEIGHT && piece.size.width < WIDTH
        }));
    }

    #[test]
    fn blocks_cover_their_part_of_the_cell() {
        let pieces = pieces_of("▀▛");
        // ▛'s upper half joins ▀ beside it; its lower left quarter stays apart.
        assert_eq!(pieces.len(), 2);
        let rect = |x, y, width, height| Bounds::new(point(x, y), size(width, height));
        assert_eq!(bounds(&pieces[0]), rect(px(0.0), px(0.0), WIDTH * 2.0, HEIGHT * 0.5));
        assert_eq!(bounds(&pieces[1]), rect(WIDTH, HEIGHT * 0.5, WIDTH * 0.5, HEIGHT * 0.5));
    }

    #[test]
    fn text_without_lines_or_with_combining_marks_is_left_to_the_font() {
        assert_eq!(split("plain", 5, WIDTH, HEIGHT), None);
        assert_eq!(split("─e\u{301}", 2, WIDTH, HEIGHT), None);
    }
}
