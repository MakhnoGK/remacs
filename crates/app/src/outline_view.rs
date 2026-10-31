mod banner;
mod history;
mod rows;

use gpui::{
    AnyElement, Context, Div, IntoElement, MouseButton, MouseDownEvent, Pixels, div, prelude::*,
    px, rgba,
};
use remacs_core::{Code, Outline, OutlineRow, TextSide};
use remacs_theme::{font, glass};

use banner::render_banner;
pub use banner::render_toolbar;
use history::{render_beside_rows, render_history};
pub use rows::ref_chip;
use rows::{render_facts, render_rows};

use crate::conflict_view::render_conflict;
use crate::editor_view::EditorView;
use crate::highlights::Highlights;
use crate::preview_view::pane;
use crate::table_view::{LIST_BOTTOM, clickable};
use crate::widgets::{Scrolled, StyleExt, fade_scrolled, over, scrolled_by, shadow};

const PANE_TOP: f32 = 56.0;
const BANNER_TOP: f32 = 52.0;
const BANNER_HEIGHT: f32 = 44.0;
const BANNER_GAP: f32 = 12.0;
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
const HISTORY_GAP: f32 = 20.0;
const MIN_HISTORY_WIDTH: f32 = 420.0;
const HISTORY_HEADER_HEIGHT: f32 = 32.0;
const HISTORY_HEADER_GAP: f32 = 6.0;
pub const COMMIT_HEIGHT: f32 = 32.0;
const FIRST_LANE_X: f32 = 20.0;
const LANE_WIDTH: f32 = 18.0;
/// Where commit messages start, however few lanes there are.
const MIN_COMMIT_INDENT: f32 = 84.0;
/// Lanes drawn at most; a wider graph is cut off.
const MAX_LANES: usize = 12;
const GRAPH_STROKE: f32 = 2.0;

/// The most lines that can fit in a window this tall, before the rows are known.
pub fn max_outline_lines(window_height: Pixels) -> usize {
    (rows_height(window_height, 0, false) / DIFF_LINE_HEIGHT).ceil() as usize + 1
}

/// Where the panes start: under the banner if there is one.
fn pane_top(has_banner: bool) -> f32 {
    if has_banner { BANNER_TOP + BANNER_HEIGHT + BANNER_GAP } else { PANE_TOP }
}

/// How many of `rows`, from the first, fit under `fact_count` facts in a window this tall.
pub fn fitting_rows(
    rows: &[OutlineRow],
    fact_count: usize,
    has_banner: bool,
    window_height: Pixels,
) -> usize {
    fitting_rows_in(rows, rows_height(window_height, fact_count, has_banner))
}

/// How many of `rows`, from the first, fit in `room` pixels.
pub fn fitting_rows_in(rows: &[OutlineRow], room: f32) -> usize {
    let mut used = 0.0;
    let past_room = (0..rows.len()).find(|&index| {
        used += row_height(rows, index);
        used > room
    });
    past_room.map_or(rows.len(), |index| index.max(1))
}

/// The height `rows[index]` takes in the pane, the gap after it and a diff's frame
/// included; 0 past the end.
pub fn row_height(rows: &[OutlineRow], index: usize) -> f32 {
    let is_diff = |row: Option<&OutlineRow>| matches!(row, Some(OutlineRow::Diff(_)));
    let Some(row) = rows.get(index) else {
        return 0.0;
    };
    let mut height = match row {
        OutlineRow::Heading { .. } => HEADING_HEIGHT + ROW_GAP,
        OutlineRow::File { .. } | OutlineRow::Item { .. } => ROW_HEIGHT + ROW_GAP,
        OutlineRow::Diff(_) => DIFF_LINE_HEIGHT,
    };
    if is_diff(Some(row)) && !is_diff(index.checked_sub(1).and_then(|before| rows.get(before))) {
        height += DIFF_TOP + ROW_GAP;
    }
    if is_diff(Some(row)) && !is_diff(rows.get(index + 1)) {
        height += DIFF_BOTTOM;
    }
    height
}

/// The room between the top of the panes and the bottom of the window.
fn panes_height(window_height: Pixels, has_banner: bool) -> f32 {
    (window_height / px(1.0)) - pane_top(has_banner) - LIST_BOTTOM
}

fn rows_height(window_height: Pixels, fact_count: usize, has_banner: bool) -> f32 {
    let facts = if fact_count == 0 {
        0.0
    } else {
        FACTS_TOP + FACT_HEIGHT * fact_count as f32 + FACTS_BOTTOM
    };
    panes_height(window_height, has_banner) - 2.0 * PANE_PADDING - facts
}

/// The room under the tabs of the column at the right, for its rows or its commits.
pub fn beside_height(window_height: Pixels, has_banner: bool) -> f32 {
    panes_height(window_height, has_banner) - HISTORY_HEADER_HEIGHT - HISTORY_HEADER_GAP
}

/// Whether a window this wide has room for the history beside the pane.
pub fn fits_history(window_width: Pixels) -> bool {
    fits_beside(window_width, PANE_WIDTH)
}

/// Whether a pane `width` wide leaves room beside it for the history or the text.
pub fn fits_beside(window_width: Pixels, width: f32) -> bool {
    window_width >= px(2.0 * PANE_SIDE + width + HISTORY_GAP + MIN_HISTORY_WIDTH)
}

/// How many commits fit under the history's header in a window this tall.
pub fn fitting_commits(window_height: Pixels, has_banner: bool) -> usize {
    let commits = beside_height(window_height, has_banner) / COMMIT_HEIGHT;
    commits.floor().max(0.0) as usize
}

/// Where the text starts, past a side column `width` wide.
pub fn text_side_end(width: f32) -> Pixels {
    px(PANE_SIDE + width + HISTORY_GAP)
}

/// Whether the outline's rows and its history go on beyond what is shown, and how far
/// their first row is scrolled past, in pixels.
pub struct OutlineScrolled {
    pub rows: Scrolled,
    pub rows_shift: f32,
    pub history: Scrolled,
    pub history_shift: f32,
}

/// The outline's facts and its rows from `first_line` on, point's line selected, in a
/// pane at the left of the window; beside it the history or a file's conflicts, and the
/// banner over both. With a side, the side goes in the pane and the rows beside it.
pub fn render_outline(
    outline: Outline,
    first_line: usize,
    point_line: usize,
    scrolled: OutlineScrolled,
    window_height: Pixels,
    code_faces: &dyn Fn(&Code) -> Highlights,
    cx: &mut Context<EditorView>,
) -> impl IntoElement {
    let (toggle, activate) = (outline.toggle, outline.activate);
    let has_banner = outline.banner.is_some();
    let banner = outline.banner.map(|banner| render_banner(banner, cx));
    let lines = (first_line..).zip(outline.rows);
    let rows = render_rows(lines, Some(point_line), |row, line, is_heading| {
        row.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                if is_heading {
                    view.click_heading(line, toggle, cx);
                } else {
                    view.click_row(line, event.click_count, activate, cx);
                }
            }),
        )
    });
    let conflict = outline.conflict.map(|conflict| render_conflict(conflict, code_faces, cx));
    let (pane, side) = match outline.side {
        Some(mut side) => {
            side.facts = outline.facts;
            let (scrolled, shift) = (scrolled.rows, scrolled.rows_shift);
            let beside = conflict
                .unwrap_or_else(|| render_beside_rows(outline.tabs, rows, scrolled, shift, cx));
            let pane = side_pane(side, PANE_WIDTH, window_height, has_banner, cx);
            (pane, Some(beside))
        }
        None => {
            let facts = render_facts(outline.facts, cx);
            let side = conflict.or_else(|| {
                let (scrolled, shift) = (scrolled.history, scrolled.history_shift);
                let history = outline.history;
                history.map(|history| render_history(history, outline.tabs, scrolled, shift, cx))
            });
            let (scrolled, shift) = (scrolled.rows, scrolled.rows_shift);
            (pane_column(facts, rows, scrolled, shift, PANE_WIDTH), side)
        }
    };
    let panes = between_sides(pane_top(has_banner)).bottom(px(LIST_BOTTOM)).flex();
    let panes = panes.gap(px(HISTORY_GAP)).child(pane).children(side);
    div().absolute().inset_0().children(banner).child(panes)
}

/// Out of the flow from `top` down, `PANE_SIDE` in from the window's sides.
fn between_sides(top: f32) -> Div {
    div().absolute().top(px(top)).left(px(PANE_SIDE)).right(px(PANE_SIDE))
}

/// Children in a row, centered across it, that keeps its size: `height` tall, padded
/// `padding_x` at the sides and rounded by `radius`.
pub(crate) fn chip_frame(height: f32, padding_x: f32, radius: f32) -> Div {
    div().flex().flex_none().items_center().h(px(height)).px(px(padding_x)).rounded(px(radius))
}

/// `row` with the selection's background, lifted, while it is selected.
pub(crate) fn lift_selected(row: Div, selected: bool) -> Div {
    row.when(selected, |row| {
        row.bg(rgba(glass::selection())).shadow(vec![shadow(glass::shadow(0x73), 1.0, 3.0, 0.0)])
    })
}

/// `rows` in a column, moved up by `shift` pixels.
fn shifted(rows: Vec<AnyElement>, shift: f32) -> Div {
    scrolled_by(div().column().gap(px(ROW_GAP)).children(rows), shift)
}

/// A pane `width` wide: `facts` over `rows`, the rows moved up by `shift` pixels.
fn pane_column(
    facts: Option<Div>,
    rows: Vec<AnyElement>,
    scrolled: Scrolled,
    shift: f32,
    width: f32,
) -> Div {
    let under = over(glass::pane(), glass::window_tint());
    let pane = pane().w(px(width)).p(px(PANE_PADDING)).gap(px(ROW_GAP));
    let pane = pane.ui().text_size(px(font::ui_size())).children(facts);
    pane.child(fade_scrolled(shifted(rows, shift), scrolled, under))
}

/// The column a plugin puts at the left of a buffer drawn as text, `width` wide, in a
/// window `height` tall above any pane; its selected row is kept in sight.
pub fn render_text_side(
    side: TextSide,
    width: f32,
    height: Pixels,
    cx: &mut Context<EditorView>,
) -> AnyElement {
    let column = div().absolute().top(px(PANE_TOP)).left(px(PANE_SIDE)).bottom(px(LIST_BOTTOM));
    column
        .flex()
        // Clicks between rows stay off the text.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(side_pane(side, width, height, false, cx))
        .into_any_element()
}

/// A side's pane `width` wide, in a window `height` tall; its selected row kept in sight.
fn side_pane(
    side: TextSide,
    width: f32,
    height: Pixels,
    has_banner: bool,
    cx: &mut Context<EditorView>,
) -> Div {
    let (rows, actions, fact_count) = (side.rows, side.actions, side.facts.len());
    let first = first_shown(&rows, fact_count, has_banner, height, side.selected);
    let shown = fitting_rows(&rows[first..], fact_count, has_banner, height);
    let scrolled = Scrolled { above: first > 0, below: first + shown < rows.len() };
    let facts = render_facts(side.facts, cx);
    let shown_rows = (first..).zip(rows[first..first + shown].iter().cloned());
    let rows = render_rows(shown_rows, side.selected, |row, index, _| {
        let action = actions[index].clone();
        row.when_some(action, |row, action| clickable(row, action, cx))
    });
    pane_column(facts, rows, scrolled, 0.0, width)
}

/// The first of `rows` shown under `fact_count` facts in a window `height` tall: the
/// first that keeps `selected` in sight.
fn first_shown(
    rows: &[OutlineRow],
    fact_count: usize,
    has_banner: bool,
    height: Pixels,
    selected: Option<usize>,
) -> usize {
    let Some(selected) = selected else {
        return 0;
    };
    let fitting = |first: usize| fitting_rows(&rows[first..], fact_count, has_banner, height);
    // No more rows than this fit, so none further back can be the first.
    let mut first = selected.saturating_sub(max_outline_lines(height));
    while first < selected && selected >= first + fitting(first) {
        first += 1;
    }
    first
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use remacs_core::{FileState, Tone};

    use super::*;

    /// A heading over `files` file rows, as a commit's side column has them.
    fn file_rows(files: usize) -> Vec<OutlineRow> {
        let heading = OutlineRow::Heading {
            title: "Files".into(),
            count: files,
            open: true,
            tone: Tone::Plain,
        };
        let file = |index: usize| OutlineRow::File {
            state: FileState::Added,
            name: format!("file_{index}.rs"),
            directory: "vendor/".into(),
            lines: None,
            note: None,
            open: false,
        };
        [heading].into_iter().chain((0..files).map(file)).collect()
    }

    #[test]
    fn the_selected_row_is_the_last_shown_once_past_the_first_screen() {
        let rows = file_rows(3000);
        let height = px(900.0);
        let fit = fitting_rows(&rows, 3, false, height);
        assert_eq!(first_shown(&rows, 3, false, height, None), 0);
        assert_eq!(first_shown(&rows, 3, false, height, Some(fit - 1)), 0);
        for selected in [fit, 1500, 3000] {
            let first = first_shown(&rows, 3, false, height, Some(selected));
            let shown = fitting_rows(&rows[first..], 3, false, height);
            assert_eq!(first + shown, selected + 1);
        }
        let start = Instant::now();
        for _ in 0..100 {
            std::hint::black_box(first_shown(&rows, 3, false, height, Some(3000)));
        }
        let elapsed = start.elapsed() / 100;
        eprintln!("first_shown, last of 3000 rows: {elapsed:?}");
    }
}
