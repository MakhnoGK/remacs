use std::ops::{Range, RangeInclusive};

use gpui::{
    AnyElement, App, Bounds, Corners, Hsla, PathBuilder, Pixels, Window, canvas, fill, point,
    prelude::*, px, rgba, size,
};
use remacs_core::{Buffer, Cursor};
use remacs_theme::glass;

use super::EditorView;
use crate::display_line::DisplayLine;

/// Points along a concave corner's quarter circle.
const FILLET_STEPS: usize = 6;

/// Part of a row's region: a block with its corners rounded, or the fill of an inside
/// corner where a longer row above or below meets it.
#[derive(Debug, PartialEq)]
enum Piece {
    /// Top left, top right, bottom right, bottom left radii.
    Block { left: f32, width: f32, radii: [f32; 4] },
    /// From `corner`, `toward` the side and the vertical it fills.
    Fillet { corner: (f32, f32), toward: (f32, f32), radius: f32 },
}

impl EditorView {
    /// Display columns of the line the regions cover on `display_line`, merged; none in
    /// another window. Counted from the line's start, not the part drawn, so that rows cut
    /// in a tab when scrolled sideways compare as they are drawn.
    pub(super) fn region_spans(
        &self,
        line: usize,
        display_line: &mut DisplayLine,
        rectangle: Option<&(RangeInclusive<usize>, Range<usize>)>,
    ) -> Vec<Range<usize>> {
        if !self.drawing_selected() {
            return Vec::new();
        }
        let buffer = self.editor.window_buffer();
        let line_range = buffer.text().line_range(line).unwrap_or_default();
        let rectangle =
            rectangle.and_then(|rectangle| rectangle_columns(buffer, rectangle, line, &line_range));
        let first_column = display_line.first_column();
        let spans = (buffer.region().filter(|_| !buffer.is_rectangle()).into_iter())
            .chain(buffer.cursors().iter().filter_map(Cursor::region))
            .filter_map(|region| region_columns(buffer, region, line))
            .chain(rectangle)
            .map(|columns| {
                let cells = display_line.cell_range(columns);
                let column = |byte| first_column + display_line.text[..byte].chars().count();
                column(cells.start)..column(cells.end)
            })
            .filter(|span| !span.is_empty())
            .collect();
        merge(spans)
    }
}

/// `spans` in order, those that overlap or touch made one: the region's color is
/// translucent, and drawn twice it would be darker.
fn merge(mut spans: Vec<Range<usize>>) -> Vec<Range<usize>> {
    spans.sort_by_key(|span| span.start);
    spans.dedup_by(|next, merged| {
        let touches = next.start <= merged.end;
        if touches {
            merged.end = merged.end.max(next.end);
        }
        touches
    });
    spans
}

/// The region on a row of `this` spans, drawn from its `first_column`, with rounded
/// corners where the rows `above` and `below` do not go on.
pub(super) fn region_shape(
    rows: [&[Range<usize>]; 3],
    first_column: usize,
    char_width: Pixels,
    row_height: Pixels,
    radius: f32,
) -> Option<AnyElement> {
    if rows[1].is_empty() {
        return None;
    }
    let (char_width, row_height) = (char_width.into(), row_height.into());
    let pieces = pieces(rows, first_column, char_width, row_height, radius);
    let color: Hsla = rgba(glass::region_tint()).into();
    let paint = move |bounds: Bounds<Pixels>, (), window: &mut Window, _: &mut App| {
        let at = |x: f32, y: f32| bounds.origin + point(px(x), px(y));
        for piece in &pieces {
            match *piece {
                Piece::Block {
                    left,
                    width,
                    radii: [top_left, top_right, bottom_right, bottom_left],
                } => {
                    let block = Bounds::new(at(left, 0.0), size(px(width), px(row_height)));
                    let radii = Corners { top_left, top_right, bottom_right, bottom_left };
                    window.paint_quad(fill(block, color).corner_radii(radii.map(|r| px(*r))));
                }
                Piece::Fillet { corner: (x, y), toward: (dx, dy), radius } => {
                    let center = (x + dx * radius, y + dy * radius);
                    let arc = (0..=FILLET_STEPS).map(|step| {
                        let angle = std::f32::consts::FRAC_PI_2 * step as f32 / FILLET_STEPS as f32;
                        let (sin, cos) = angle.sin_cos();
                        at(center.0 - dx * radius * sin, center.1 - dy * radius * cos)
                    });
                    let mut path = PathBuilder::fill();
                    path.add_polygon(&[at(x, y)].into_iter().chain(arc).collect::<Vec<_>>(), true);
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                }
            }
        }
    };
    Some(canvas(|_, _, _| {}, paint).absolute().inset_0().into_any_element())
}

/// Characters of `line` inside `region`; past the end means the newline too.
pub(super) fn region_columns(
    buffer: &Buffer,
    region: Range<usize>,
    line: usize,
) -> Option<Range<usize>> {
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

fn pieces(
    [above, this, below]: [&[Range<usize>]; 3],
    first_column: usize,
    char_width: f32,
    row_height: f32,
    radius: f32,
) -> Vec<Piece> {
    let x_of = |column: usize| (column - first_column) as f32 * char_width;
    let mut pieces = Vec::new();
    for (index, span) in this.iter().enumerate() {
        let (left, right) = (x_of(span.start), x_of(span.end));
        // Half the gap to the spans beside it on the row, so that their fillets do not meet.
        let room_left = index
            .checked_sub(1)
            .map_or(f32::MAX, |before| (span.start - this[before].end) as f32 * char_width / 2.0);
        let room_right = this
            .get(index + 1)
            .map_or(f32::MAX, |after| (after.start - span.end) as f32 * char_width / 2.0);
        let round = radius.min((right - left) / 2.0).min(row_height / 2.0);
        let mut radii = [round; 4];
        // Each edge with its corners' radii, its height, the way into the row.
        for (neighbors, [left_radius, right_radius], y, dy) in
            [(above, [0, 1], 0.0, 1.0), (below, [3, 2], row_height, -1.0)]
        {
            // How far the neighbor that goes on over each corner reaches past it.
            let left_past = neighbors
                .iter()
                .find(|n| n.start <= span.start && span.start < n.end)
                .map(|n| span.start - n.start);
            let right_past = neighbors
                .iter()
                .find(|n| n.start < span.end && span.end <= n.end)
                .map(|n| n.end - span.end);
            for (corner, x, dx, past, room) in [
                (left_radius, left, -1.0, left_past, room_left),
                (right_radius, right, 1.0, right_past, room_right),
            ] {
                let Some(past) = past else { continue };
                radii[corner] = 0.0;
                let fillet = radius.min(row_height / 2.0).min(past as f32 * char_width).min(room);
                if fillet > 0.0 {
                    pieces.push(Piece::Fillet { corner: (x, y), toward: (dx, dy), radius: fillet });
                }
            }
        }
        pieces.push(Piece::Block { left, width: right - left, radii });
    }
    pieces
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
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
        assert_eq!(rectangle_columns(&buffer, &rectangle, 0, &(0..6)), Some(1..4));
        assert_eq!(rectangle_columns(&buffer, &rectangle, 1, &(7..9)), Some(1..2));
        assert_eq!(rectangle_columns(&buffer, &rectangle, 2, &(10..10)), None);
        assert_eq!(rectangle_columns(&buffer, &(0..=1, 3..5), 1, &(7..9)), None);
    }

    const WIDTH: f32 = 10.0;
    const HEIGHT: f32 = 20.0;

    fn row(above: &[Range<usize>], this: &[Range<usize>], below: &[Range<usize>]) -> Vec<Piece> {
        pieces([above, this, below], 0, WIDTH, HEIGHT, 4.0)
    }

    #[test]
    fn merge_joins_spans_that_overlap_or_touch_and_keeps_the_rest_in_order() {
        assert_eq!(merge(vec![6..7, 3..5, 0..2, 1..4, 9..10, 7..8]), [0..5, 6..8, 9..10]);
        assert_eq!(merge(vec![2..9, 3..4]), [2..9]);
    }

    #[test]
    fn rows_cut_at_different_columns_compare_by_the_line_and_draw_from_their_own_start() {
        // Scrolled sideways to column 2, the row above cut in a tab draws from column 1; its
        // span counts from the line's start, so the two line up as they are drawn.
        assert_eq!(
            pieces([&[2..6], &[2..6], &[]], 2, WIDTH, HEIGHT, 4.0),
            [Piece::Block { left: 0.0, width: 40.0, radii: [0.0, 0.0, 4.0, 4.0] }]
        );
    }

    #[test]
    fn fillets_facing_each_other_share_the_gap_between_spans() {
        let fillet = |x, dx, radius| Piece::Fillet { corner: (x, 0.0), toward: (dx, 1.0), radius };
        let block = |left, width, radii| Piece::Block { left, width, radii };
        assert_eq!(
            pieces([&[0..9], &[0..2, 3..5], &[]], 0, WIDTH, HEIGHT, 8.0),
            [
                fillet(20.0, 1.0, 5.0),
                block(0.0, 20.0, [0.0, 0.0, 8.0, 8.0]),
                fillet(30.0, -1.0, 5.0),
                fillet(50.0, 1.0, 8.0),
                block(30.0, 20.0, [0.0, 0.0, 8.0, 8.0])
            ]
        );
    }

    #[test]
    fn a_lone_row_is_rounded_at_every_corner() {
        assert_eq!(
            row(&[], &[2..5], &[]),
            [Piece::Block { left: 20.0, width: 30.0, radii: [4.0; 4] }]
        );
    }

    #[test]
    fn a_short_row_between_longer_ones_fills_its_inside_corners() {
        let fillet = |y, dy| Piece::Fillet { corner: (10.0, y), toward: (1.0, dy), radius: 4.0 };
        assert_eq!(
            row(&[0..7], &[0..1], &[0..4]),
            [
                fillet(0.0, 1.0),
                fillet(HEIGHT, -1.0),
                Piece::Block { left: 0.0, width: 10.0, radii: [0.0; 4] }
            ]
        );
    }

    #[test]
    fn a_longer_row_rounds_the_corners_past_its_neighbors() {
        assert_eq!(
            row(&[0..1], &[0..7], &[3..9]),
            [
                Piece::Fillet { corner: (70.0, HEIGHT), toward: (1.0, -1.0), radius: 4.0 },
                Piece::Block { left: 0.0, width: 70.0, radii: [0.0, 4.0, 0.0, 4.0] }
            ]
        );
    }

    #[test]
    fn a_narrow_block_rounds_by_half_its_width_at_most() {
        let pieces = pieces([&[], &[0..1], &[]], 0, 6.0, HEIGHT, 8.0);
        assert_eq!(pieces, [Piece::Block { left: 0.0, width: 6.0, radii: [3.0; 4] }]);
    }
}
