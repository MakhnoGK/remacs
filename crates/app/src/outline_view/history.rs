use gpui::{
    AnyElement, BorderStyle, Bounds, Context, Div, PathBuilder, Pixels, Point, ScrollWheelEvent,
    Window, canvas, div, point, prelude::*, px, quad, rgb, rgba, size,
};
use remacs_core::{Button, CommitRow, GraphNode, GraphRow, History};
use remacs_theme::{color, font, glass};

use super::{
    COMMIT_HEIGHT, FIRST_LANE_X, GRAPH_STROKE, HISTORY_HEADER_GAP, HISTORY_HEADER_HEIGHT,
    LANE_WIDTH, MAX_LANES, MIN_COMMIT_INDENT, chip_frame, ref_chip, shifted,
};
use crate::editor_view::EditorView;
use crate::table_view::clickable;
use crate::widgets::{Scrolled, StyleExt, fade_scrolled, kbd, scrolled_by, segment, segmented};

/// The column at the right of the pane, its tabs atop it.
fn beside_column() -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(HISTORY_HEADER_GAP))
        .overflow_hidden()
        .font_family(font::ui())
        .text_size(px(font::ui_size()))
}

/// `list` dissolving into the window at the edges with more beyond them.
fn fade_beside(list: Div, scrolled: Scrolled) -> Div {
    fade_scrolled(list, scrolled, rgba(glass::window_tint()).into())
}

/// The tabs atop the column at the right; a click on one switches to it.
fn tabs_header(tabs: Vec<Button>, cx: &mut Context<EditorView>) -> Div {
    let tabs = tabs.into_iter();
    let tabs = tabs.map(|tab| clickable(segment(&tab.label, tab.selected), tab.action, cx));
    div()
        .row(12.0)
        .flex_none()
        .h(px(HISTORY_HEADER_HEIGHT))
        .pl(px(12.0))
        .pr(px(4.0))
        .child(segmented().children(tabs))
}

/// An outline's rows at the right under its tabs, moved up by `shift` pixels.
pub(super) fn render_beside_rows(
    tabs: Vec<Button>,
    rows: Vec<AnyElement>,
    scrolled: Scrolled,
    shift: f32,
    cx: &mut Context<EditorView>,
) -> Div {
    beside_column().child(tabs_header(tabs, cx)).child(fade_beside(shifted(rows, shift), scrolled))
}

pub(super) fn render_history(
    history: History,
    tabs: Vec<Button>,
    scrolled: Scrolled,
    shift: f32,
    cx: &mut Context<EditorView>,
) -> Div {
    let lanes = (history.commits.iter())
        .flat_map(|commit| {
            let graph = &commit.graph;
            let edges = graph.upper.iter().chain(&graph.lower);
            let ends = edges.flat_map(|edge| [edge.from, edge.to]);
            ends.chain([graph.lane])
        })
        .max()
        .map_or(1, |lane| lane + 1)
        .min(MAX_LANES);
    let indent = (FIRST_LANE_X + LANE_WIDTH * (lanes - 1) as f32 + 28.0).max(MIN_COMMIT_INDENT);
    let search_hint = match history.search {
        Some(search) => div().text_color(rgb(color::fg_alt())).child(format!("“{search}”")),
        None => div().child("Search history"),
    };
    let search = div().row(6.0).px(px(8.0)).status_text(color::fg_faint());
    let header = tabs_header(tabs, cx)
        .children(history.file.map(|file| file_chip(file, cx)))
        .child(div().flex_1())
        .child(search.child(kbd("/", false)).child(search_hint));
    let list = if history.commits.is_empty() {
        let empty = div().pl(px(12.0)).pt(px(8.0)).child(history.empty);
        empty.text_color(rgb(color::fg_faint()))
    } else {
        let selected = history.selected;
        let commits = history.commits.into_iter().enumerate();
        let commits =
            commits.map(|(index, commit)| commit_row(commit, indent, selected == Some(index), cx));
        let commits = div().flex().flex_col().children(commits);
        fade_beside(scrolled_by(commits, shift), scrolled)
    };
    beside_column()
        .on_scroll_wheel(cx.listener(move |view, event: &ScrollWheelEvent, _, cx| {
            cx.stop_propagation();
            view.scroll_history(event, cx);
        }))
        .child(header)
        .child(list)
}

/// The file a history shows the commits of, with what clears it, which a click runs.
fn file_chip(file: Button, cx: &mut Context<EditorView>) -> Div {
    let chip = chip_frame(22.0, 8.0, 6.0)
        .gap(px(6.0))
        .bg(rgba(glass::chip()))
        .font_family(font::mono())
        .text(font::small_size(), color::fg_body())
        .child(div().max_w(px(220.0)).truncate().child(file.label))
        .child(div().text_color(rgb(color::fg_faint())).child("×"));
    clickable(chip, file.action, cx)
}

/// One of a commit's columns after its subject, `width` wide, showing `text`.
fn commit_column(width: Pixels, color: u32, text: String) -> Div {
    div().flex_none().w(width).status_text(color).child(text)
}

fn commit_row(commit: CommitRow, indent: f32, selected: bool, cx: &mut Context<EditorView>) -> Div {
    let graph = commit.graph;
    let row = div()
        .relative()
        .row(12.0)
        .flex_none()
        .h(px(COMMIT_HEIGHT))
        .pl(px(indent))
        .pr(px(12.0))
        .rounded(px(8.0))
        .when(selected, |row| row.bg(rgba(glass::selection())))
        .whitespace_nowrap();
    let graph =
        canvas(|_, _, _| {}, move |bounds, _, window, _| paint_graph(&graph, bounds, window));
    let subject = div()
        .row(6.0)
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .text_color(rgb(color::fg_main()))
        .children(commit.refs.into_iter().map(ref_chip))
        .child(div().min_w_0().truncate().child(commit.subject));
    let author = commit_column(px(112.0), color::fg_dim(), commit.author).truncate();
    let date = commit_column(px(84.0), color::fg_faint(), commit.date).text_right();
    let id = commit_column(px(64.0), color::fg_line_number(), commit.id);
    clickable(row, commit.open, cx)
        .child(graph.absolute().top_0().bottom_0().left_0().w(px(indent)))
        .child(subject)
        .child(author)
        .child(date)
        .child(id.text_right().font_family(font::mono()))
}

/// The lines through a commit's row, curving where they change lanes, and its node.
fn paint_graph(graph: &GraphRow, bounds: Bounds<Pixels>, window: &mut Window) {
    let x = |lane: usize| bounds.left() + px(FIRST_LANE_X + LANE_WIDTH * lane as f32);
    let top = bounds.top();
    let middle = top + px(COMMIT_HEIGHT / 2.0);
    let bottom = top + px(COMMIT_HEIGHT);
    for (edges, from_y, to_y) in [(&graph.upper, top, middle), (&graph.lower, middle, bottom)] {
        for edge in edges.iter().filter(|edge| edge.from.max(edge.to) < MAX_LANES) {
            let start = point(x(edge.from), from_y);
            let end = point(x(edge.to), to_y);
            let mut path = PathBuilder::stroke(px(GRAPH_STROKE));
            path.move_to(start);
            if edge.from == edge.to {
                path.line_to(end);
            } else {
                let bend = (to_y - from_y) * 0.6;
                let control_a = point(start.x, from_y + bend);
                path.cubic_bezier_to(end, control_a, point(end.x, to_y - bend));
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, rgb(color::graph(edge.color)));
            }
        }
    }
    if graph.lane >= MAX_LANES {
        return;
    }
    let center = point(x(graph.lane), middle);
    let lane_color = color::graph(graph.color);
    match graph.node {
        GraphNode::Head => {
            paint_dot(window, center, 6.0, color::bg_main(), lane_color);
            paint_dot(window, center, 1.5, lane_color, lane_color);
        }
        GraphNode::Merge => paint_dot(window, center, 5.5, color::bg_main(), lane_color),
        GraphNode::Commit => paint_dot(window, center, 4.5, lane_color, color::bg_main()),
    }
}

/// A circle of `radius` to the middle of its ring, as SVG strokes it.
fn paint_dot(window: &mut Window, center: Point<Pixels>, radius: f32, fill: u32, ring: u32) {
    let outer = px(radius + GRAPH_STROKE / 2.0);
    let origin = point(center.x - outer, center.y - outer);
    let bounds = Bounds::new(origin, size(outer * 2.0, outer * 2.0));
    let (fill, ring) = (rgb(fill), rgb(ring));
    window.paint_quad(quad(bounds, outer, fill, px(GRAPH_STROKE), ring, BorderStyle::Solid));
}
