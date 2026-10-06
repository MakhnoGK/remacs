use gpui::{
    AnyElement, BorderStyle, Bounds, Context, Div, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, PathBuilder, Pixels, Point, ScrollWheelEvent, Window, canvas,
    div, point, prelude::*, px, quad, rgb, rgba, size, svg,
};
use remacs_core::{
    Banner, BannerButton, BannerPart, Code, CommitRow, DiffRow, Fact, FactValue, GraphNode,
    GraphRow, History, Outline, OutlineRow, RefChip, RefKind, Tone, ToolbarButton,
};
use remacs_theme::{color, font, glass};

use crate::assets::{BRANCH_ICON, CHEVRON_DOWN_ICON, CHEVRON_ICON};
use crate::conflict_view::render_conflict;
use crate::editor_view::EditorView;
use crate::highlights::Highlights;
use crate::table_view::{LIST_BOTTOM, clickable};
use crate::widgets::{
    Scrolled, fade_scrolled, icon, kbd, line_counts, over, scrolled_by, segment, segmented, shadow,
    state_letter, tone_color,
};

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
    if has_banner {
        BANNER_TOP + BANNER_HEIGHT + BANNER_GAP
    } else {
        PANE_TOP
    }
}

/// How many of `rows`, from the first, fit under `fact_count` facts in a window this tall.
pub fn fitting_rows(
    rows: &[OutlineRow],
    fact_count: usize,
    has_banner: bool,
    window_height: Pixels,
) -> usize {
    let room = rows_height(window_height, fact_count, has_banner);
    let mut used = 0.0;
    for index in 0..rows.len() {
        used += row_height(rows, index);
        if used > room {
            return index.max(1);
        }
    }
    rows.len()
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

fn rows_height(window_height: Pixels, fact_count: usize, has_banner: bool) -> f32 {
    let facts = if fact_count == 0 {
        0.0
    } else {
        FACTS_TOP + FACT_HEIGHT * fact_count as f32 + FACTS_BOTTOM
    };
    (window_height / px(1.0)) - pane_top(has_banner) - LIST_BOTTOM - 2.0 * PANE_PADDING - facts
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
/// banner over both.
pub fn render_outline(
    outline: Outline,
    first_line: usize,
    point_line: usize,
    scrolled: OutlineScrolled,
    code_faces: &dyn Fn(&Code) -> Highlights,
    cx: &mut Context<EditorView>,
) -> impl IntoElement {
    let Outline {
        facts,
        rows,
        toggle,
        activate,
        history,
        conflict,
        banner,
        ..
    } = outline;
    let has_banner = banner.is_some();
    let banner = banner.map(|banner| render_banner(banner, cx));
    let facts = (!facts.is_empty()).then(|| render_facts(facts, cx));
    let mut children: Vec<AnyElement> = Vec::new();
    let mut diff_block: Vec<AnyElement> = Vec::new();
    let row_count = rows.len();
    for (index, row) in rows.into_iter().enumerate() {
        let line = first_line + index;
        let selected = line == point_line;
        let element = match row {
            OutlineRow::Diff(diff) => {
                diff_block.push(render_diff_line(diff, selected, line, activate, cx));
                None
            }
            OutlineRow::Heading {
                title,
                count,
                open,
                tone,
            } => Some(
                render_heading(&title, count, open, tone, selected).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        view.click_heading(line, toggle, cx);
                    }),
                ),
            ),
            OutlineRow::File {
                state,
                name,
                directory,
                lines,
                note,
                ..
            } => Some(clicks(
                row_frame(selected)
                    .child(state_letter(Some(state), selected).w(px(12.0)))
                    .child(div().flex_none().font_family(font::mono()).child(name))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(font::STATUS_SIZE))
                            .text_color(rgb(if selected {
                                color::FG_ALT
                            } else {
                                color::FG_FAINT
                            }))
                            .child(directory),
                    )
                    .map(|row| match note {
                        Some((note, tone)) => row.child(
                            div()
                                .flex_none()
                                .text_size(px(font::STATUS_SIZE))
                                .text_color(rgb(tone_color(tone, color::FG_DIM)))
                                .child(note),
                        ),
                        None => row.children(lines.map(|lines| line_counts(lines, selected))),
                    }),
                line,
                activate,
                cx,
            )),
            OutlineRow::Item { name, description } => Some(clicks(
                row_frame(selected)
                    .child(
                        div()
                            .flex_none()
                            .font_family(font::mono())
                            .text_size(px(font::STATUS_SIZE))
                            .text_color(rgb(if selected {
                                color::FG_ALT
                            } else {
                                color::FG_DIM
                            }))
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(rgb(color::FG_BODY))
                            .child(description),
                    ),
                line,
                activate,
                cx,
            )),
        };
        if !diff_block.is_empty() && (element.is_some() || index + 1 == row_count) {
            children.push(diff_frame(std::mem::take(&mut diff_block)).into_any_element());
        }
        children.extend(element.map(IntoElement::into_any_element));
    }
    let side = match conflict {
        Some(conflict) => Some(render_conflict(conflict, code_faces, cx)),
        None => history
            .map(|history| render_history(history, scrolled.history, scrolled.history_shift, cx)),
    };
    let rows = scrolled_by(
        div().flex().flex_col().gap(px(ROW_GAP)).children(children),
        scrolled.rows_shift,
    );
    let pane = div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(PANE_WIDTH))
        .p(px(PANE_PADDING))
        .gap(px(ROW_GAP))
        .overflow_hidden()
        .bg(rgba(glass::PANE))
        .border_1()
        .border_color(rgba(glass::PANE_BORDER))
        .rounded(px(12.0))
        .font_family(font::UI)
        .text_size(px(font::UI_SIZE))
        .children(facts)
        .child(fade_scrolled(
            rows,
            scrolled.rows,
            over(glass::PANE, glass::WINDOW_TINT),
        ));
    let panes = div()
        .absolute()
        .top(px(pane_top(has_banner)))
        .left(px(PANE_SIDE))
        .right(px(PANE_SIDE))
        .bottom(px(LIST_BOTTOM))
        .flex()
        .gap(px(HISTORY_GAP))
        .child(pane)
        .children(side);
    div().absolute().inset_0().children(banner).child(panes)
}

/// What is in progress across the top: its name and refs, the step, the commit applied,
/// how far the conflicts are, and the buttons to go on.
fn render_banner(banner: Banner, cx: &mut Context<EditorView>) -> Div {
    let Banner {
        title,
        parts,
        step,
        commit,
        state: (state, state_tone),
        buttons,
    } = banner;
    let parts = parts.into_iter().map(|part| match part {
        BannerPart::Chip(chip) => ref_chip(chip),
        BannerPart::Word(word) => div().text_color(rgb(color::FG_DIM)).child(word),
    });
    let buttons: Vec<Div> = buttons
        .into_iter()
        .map(|button| render_banner_button(button, cx))
        .collect();
    div()
        .absolute()
        .top(px(BANNER_TOP))
        .left(px(PANE_SIDE))
        .right(px(PANE_SIDE))
        .h(px(BANNER_HEIGHT))
        .flex()
        .items_center()
        .gap(px(10.0))
        .pl(px(14.0))
        .pr(px(8.0))
        .rounded(px(10.0))
        .bg(rgba(color::WARNING << 8 | 0x12))
        .border_1()
        .border_color(rgba(color::WARNING << 8 | 0x47))
        .font_family(font::UI)
        .text_size(px(font::UI_SIZE))
        .text_color(rgb(color::FG_BODY))
        .whitespace_nowrap()
        .child(
            svg()
                .path(BRANCH_ICON)
                .flex_none()
                .size(px(16.0))
                .text_color(rgb(color::WARNING)),
        )
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(color::FG_MAIN))
                .child(title),
        )
        .children(parts)
        .children(step.map(|step| {
            div()
                .px(px(8.0))
                .py(px(2.0))
                .rounded(px(5.0))
                .bg(rgba(color::WARNING << 8 | 0x24))
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_ON_WARNING))
                .child(step)
        }))
        .children(commit.map(|(id, subject)| {
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_none()
                        .font_family(font::mono())
                        .text_size(px(font::STATUS_SIZE))
                        .text_color(rgb(color::FG_FAINT))
                        .child(id),
                )
                .child(div().min_w_0().truncate().child(subject))
        }))
        .child(div().flex_1())
        .child(
            div()
                .flex_none()
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(tone_color(state_tone, color::FG_DIM)))
                .child(state),
        )
        .children(buttons)
}

/// Danger for one that throws work away, Accent for the one to press next; a disabled
/// one cannot be pressed.
fn render_banner_button(button: BannerButton, cx: &mut Context<EditorView>) -> Div {
    let BannerButton {
        label,
        tone,
        enabled,
        action,
    } = button;
    let (background, border, text) = match tone {
        _ if !enabled => (glass::DISABLED, 0xffffff14, color::FG_MUTED),
        Tone::Danger => (glass::CONTROL, color::ERROR << 8 | 0x4d, color::FG_FLAGGED),
        Tone::Accent => (glass::SELECTION, color::ACCENT << 8 | 0x73, color::FG_MAIN),
        _ => (glass::CONTROL, glass::CONTROL_BORDER, color::FG_MAIN),
    };
    let element = div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(28.0))
        .px(px(12.0))
        .rounded(px(7.0))
        .bg(rgba(background))
        .border_1()
        .border_color(rgba(border))
        .text_size(px(font::STATUS_SIZE))
        .text_color(rgb(text))
        .when(enabled, |element| {
            element.shadow(vec![shadow(0x00000080, 1.0, 2.0, 0.0)])
        })
        .child(label);
    if enabled {
        clickable(element, action, cx)
    } else {
        element
    }
}

/// Buttons at the right of the title bar, with a badge for what each would move.
pub fn render_toolbar(buttons: Vec<ToolbarButton>, cx: &mut Context<EditorView>) -> Div {
    let buttons: Vec<Div> = buttons
        .into_iter()
        .map(|button| {
            let ToolbarButton {
                icon: button_icon,
                label,
                count,
                enabled,
                action,
            } = button;
            let text = if enabled {
                color::FG_BODY
            } else {
                color::FG_MUTED
            };
            let element = div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(26.0))
                .px(px(10.0))
                .rounded(px(7.0))
                .bg(rgba(glass::CONTROL))
                .border_1()
                .border_color(rgba(glass::CONTROL_BORDER))
                .shadow(vec![shadow(0x00000099, 1.0, 2.0, 0.0)])
                .font_family(font::UI)
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(text))
                .child(icon(button_icon, text))
                .child(label)
                .children(count.filter(|_| enabled).map(|count| {
                    div()
                        .px(px(5.0))
                        .rounded(px(4.0))
                        .bg(rgba(glass::SELECTION))
                        .font_family(font::mono())
                        .text_size(px(font::SMALL_SIZE))
                        .line_height(px(16.0))
                        .text_color(rgb(color::FG_MAIN))
                        .child(count.to_string())
                }));
            if enabled {
                clickable(element, action, cx)
            } else {
                element
            }
        })
        .collect();
    div()
        .absolute()
        .top(px(9.0))
        .right(px(14.0))
        .flex()
        .gap(px(8.0))
        .children(buttons)
}

/// Whether a window this wide has room for the history beside the pane.
pub fn fits_history(window_width: Pixels) -> bool {
    window_width >= px(2.0 * PANE_SIDE + PANE_WIDTH + HISTORY_GAP + MIN_HISTORY_WIDTH)
}

/// How many commits fit under the history's header in a window this tall.
pub fn fitting_commits(window_height: Pixels, has_banner: bool) -> usize {
    let room = (window_height / px(1.0))
        - pane_top(has_banner)
        - LIST_BOTTOM
        - HISTORY_HEADER_HEIGHT
        - HISTORY_HEADER_GAP;
    (room / COMMIT_HEIGHT).floor().max(0.0) as usize
}

fn render_history(
    history: History,
    scrolled: Scrolled,
    shift: f32,
    cx: &mut Context<EditorView>,
) -> Div {
    let History {
        scopes,
        search,
        commits,
        empty,
        scroll,
        ..
    } = history;
    let lanes = commits
        .iter()
        .flat_map(|commit| {
            let graph = &commit.graph;
            let edges = graph.upper.iter().chain(&graph.lower);
            edges
                .flat_map(|edge| [edge.from, edge.to])
                .chain([graph.lane])
        })
        .max()
        .map_or(1, |lane| lane + 1)
        .min(MAX_LANES);
    let indent = (FIRST_LANE_X + LANE_WIDTH * (lanes - 1) as f32 + 28.0).max(MIN_COMMIT_INDENT);
    let search_hint = match search {
        Some(search) => div()
            .text_color(rgb(color::FG_ALT))
            .child(format!("“{search}”")),
        None => div().child("Search history"),
    };
    let header =
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.0))
            .h(px(HISTORY_HEADER_HEIGHT))
            .pl(px(12.0))
            .pr(px(4.0))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(color::FG_MAIN))
                    .child("History"),
            )
            .child(segmented().children(
                scopes.into_iter().map(|scope| {
                    clickable(segment(&scope.label, scope.selected), scope.action, cx)
                }),
            ))
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .text_size(px(font::STATUS_SIZE))
                    .text_color(rgb(color::FG_FAINT))
                    .child(kbd("/", false))
                    .child(search_hint),
            );
    let list = if commits.is_empty() {
        div()
            .pl(px(12.0))
            .pt(px(8.0))
            .text_color(rgb(color::FG_FAINT))
            .child(empty)
    } else {
        let commits = div().flex().flex_col().children(
            commits
                .into_iter()
                .map(|commit| render_commit(commit, indent)),
        );
        fade_scrolled(
            scrolled_by(commits, shift),
            scrolled,
            rgba(glass::WINDOW_TINT).into(),
        )
    };
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(HISTORY_HEADER_GAP))
        .overflow_hidden()
        .font_family(font::UI)
        .text_size(px(font::UI_SIZE))
        .on_scroll_wheel(cx.listener(move |view, event: &ScrollWheelEvent, _, cx| {
            cx.stop_propagation();
            view.scroll_history(event, scroll.clone(), cx);
        }))
        .child(header)
        .child(list)
}

fn render_commit(commit: CommitRow, indent: f32) -> Div {
    let CommitRow {
        graph,
        refs,
        subject,
        author,
        date,
        id,
    } = commit;
    div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.0))
        .h(px(COMMIT_HEIGHT))
        .pl(px(indent))
        .pr(px(12.0))
        .whitespace_nowrap()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    paint_graph(&graph, bounds, window);
                },
            )
            .absolute()
            .left_0()
            .top_0()
            .w(px(indent))
            .h(px(COMMIT_HEIGHT)),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(px(6.0))
                .overflow_hidden()
                .text_color(rgb(color::FG_MAIN))
                .children(refs.into_iter().map(ref_chip))
                .child(div().min_w_0().truncate().child(subject)),
        )
        .child(
            div()
                .flex_none()
                .w(px(112.0))
                .truncate()
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_DIM))
                .child(author),
        )
        .child(
            div()
                .flex_none()
                .w(px(84.0))
                .text_right()
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_FAINT))
                .child(date),
        )
        .child(
            div()
                .flex_none()
                .w(px(64.0))
                .text_right()
                .font_family(font::mono())
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_LINE_NUMBER))
                .child(id),
        )
}

/// The lines through a commit's row, curving where they change lanes, and its node.
fn paint_graph(graph: &GraphRow, bounds: Bounds<Pixels>, window: &mut Window) {
    let x = |lane: usize| bounds.left() + px(FIRST_LANE_X + LANE_WIDTH * lane as f32);
    let top = bounds.top();
    let middle = top + px(COMMIT_HEIGHT / 2.0);
    let bottom = top + px(COMMIT_HEIGHT);
    for (edges, from_y, to_y) in [(&graph.upper, top, middle), (&graph.lower, middle, bottom)] {
        for edge in edges
            .iter()
            .filter(|edge| edge.from.max(edge.to) < MAX_LANES)
        {
            let start = point(x(edge.from), from_y);
            let end = point(x(edge.to), to_y);
            let mut path = PathBuilder::stroke(px(GRAPH_STROKE));
            path.move_to(start);
            if edge.from == edge.to {
                path.line_to(end);
            } else {
                let bend = (to_y - from_y) * 0.6;
                path.cubic_bezier_to(
                    end,
                    point(start.x, from_y + bend),
                    point(end.x, to_y - bend),
                );
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
            paint_dot(window, center, 6.0, color::BG_MAIN, lane_color);
            paint_dot(window, center, 1.5, lane_color, lane_color);
        }
        GraphNode::Merge => paint_dot(window, center, 5.5, color::BG_MAIN, lane_color),
        GraphNode::Commit => paint_dot(window, center, 4.5, lane_color, color::BG_MAIN),
    }
}

/// A circle of `radius` to the middle of its ring, as SVG strokes it.
fn paint_dot(window: &mut Window, center: Point<Pixels>, radius: f32, fill: u32, ring: u32) {
    let outer = px(radius + GRAPH_STROKE / 2.0);
    let bounds = Bounds::new(
        point(center.x - outer, center.y - outer),
        size(outer * 2.0, outer * 2.0),
    );
    window.paint_quad(quad(
        bounds,
        outer,
        rgb(fill),
        px(GRAPH_STROKE),
        rgb(ring),
        BorderStyle::Solid,
    ));
}

fn render_facts(facts: Vec<Fact>, cx: &mut Context<EditorView>) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pt(px(FACTS_TOP))
        .px(px(4.0))
        .pb(px(10.0))
        .mb(px(6.0))
        .border_b_1()
        .border_color(rgba(0xffffff0f))
        .children(facts.into_iter().map(|fact| {
            let value = match fact.value {
                FactValue::Ref(chip) => ref_chip(chip),
                FactValue::Code(text) => div()
                    .flex_none()
                    .font_family(font::mono())
                    .text_size(px(font::STATUS_SIZE))
                    .text_color(rgb(color::FG_BODY))
                    .child(text),
            };
            let (note_size, note_color) = if fact.note_is_title {
                (font::UI_SIZE, color::FG_BODY)
            } else {
                (font::STATUS_SIZE, color::FG_DIM)
            };
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .h(px(FACT_HEIGHT))
                .px(px(6.0))
                .whitespace_nowrap()
                .child(
                    div()
                        .flex_none()
                        .w(px(64.0))
                        .text_size(px(font::STATUS_SIZE))
                        .text_color(rgb(color::FG_FAINT))
                        .child(fact.label),
                )
                .child(value)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(note_size))
                        .text_color(rgb(note_color))
                        .child(fact.note),
                )
                .children(fact.buttons.into_iter().map(|button| {
                    clickable(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .h(px(24.0))
                            .px(px(10.0))
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(rgba(glass::CONTROL_BORDER))
                            .bg(rgba(glass::CONTROL))
                            .text_size(px(font::STATUS_SIZE))
                            .shadow(vec![shadow(0x00000080, 1.0, 2.0, 0.0)])
                            .child(button.label),
                        button.action,
                        cx,
                    )
                }))
        }))
}

/// A branch or tag, colored by what it is.
pub fn ref_chip(chip: RefChip) -> Div {
    let (background, foreground, border) = match chip.kind {
        RefKind::Head => (rgba(0x2fafff33), color::FG_REF_HEAD, rgba(0x2fafff73)),
        RefKind::Main => (rgba(0xb6a0ff29), color::FG_REF_MAIN, rgba(0x00000000)),
        RefKind::Branch => (rgba(0x6ae4b924), color::FG_REF_BRANCH, rgba(0x00000000)),
        RefKind::Remote => (rgba(0x00000000), color::FG_REF_REMOTE, rgba(0xffffff29)),
        RefKind::Tag => (rgba(glass::CHIP), color::FG_BODY, rgba(0x00000000)),
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(20.0))
        .px(px(7.0))
        .rounded(px(5.0))
        .border_1()
        .border_color(border)
        .bg(background)
        .font_family(font::mono())
        .text_size(px(font::SMALL_SIZE))
        .text_color(rgb(foreground))
        .child(chip.name)
}

fn render_heading(title: &str, count: usize, open: bool, tone: Tone, selected: bool) -> Div {
    let count_color = match tone {
        Tone::Faint => color::FG_LINE_NUMBER,
        tone => tone_color(tone, color::FG_LINE_NUMBER),
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .h(px(HEADING_HEIGHT))
        .px(px(10.0))
        .rounded(px(8.0))
        .when(selected, |heading| heading.bg(rgba(glass::SELECTION)))
        .text_size(px(font::SMALL_SIZE))
        .text_color(rgb(if selected {
            color::FG_ALT
        } else {
            color::FG_FAINT
        }))
        .child(
            svg()
                .path(if open {
                    CHEVRON_DOWN_ICON
                } else {
                    CHEVRON_ICON
                })
                .flex_none()
                .size(px(10.0))
                .text_color(rgb(color::FG_FAINT)),
        )
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_uppercase()),
        )
        .child(div().text_color(rgb(count_color)).child(count.to_string()))
}

fn row_frame(selected: bool) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.0))
        .h(px(ROW_HEIGHT))
        .px(px(10.0))
        .rounded(px(8.0))
        .whitespace_nowrap()
        .when(selected, |row| {
            row.bg(rgba(glass::SELECTION))
                .shadow(vec![shadow(0x00000073, 1.0, 3.0, 0.0)])
        })
}

fn clicks(row: Div, line: usize, activate: &'static str, cx: &mut Context<EditorView>) -> Div {
    row.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |view, event: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            view.click_row(line, event.click_count, activate, cx);
        }),
    )
}

fn diff_frame(lines: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .ml(px(DIFF_INDENT))
        .mr(px(4.0))
        .mt(px(2.0))
        .mb(px(6.0))
        .py(px(4.0))
        .rounded(px(8.0))
        .overflow_hidden()
        .bg(rgba(glass::WELL))
        .border_1()
        .border_color(rgba(0xffffff0f))
        .font_family(font::mono())
        .text_size(px(font::STATUS_SIZE))
        .line_height(px(DIFF_LINE_HEIGHT))
        .children(lines)
}

fn render_diff_line(
    diff: DiffRow,
    selected: bool,
    line: usize,
    activate: &'static str,
    cx: &mut Context<EditorView>,
) -> AnyElement {
    let (prefix, text, foreground, background) = match diff {
        DiffRow::Hunk(text) => ("", text, color::DOCSTRING, Some(rgba(0xffffff0a))),
        DiffRow::Context(text) => ("", text, color::FG_KBD, None),
        DiffRow::Added(text) => ("+", text, color::FG_ADDED, Some(rgba(glass::ADDED))),
        DiffRow::Removed(text) => ("-", text, color::FG_REMOVED, Some(rgba(glass::REMOVED))),
    };
    let row = div()
        .flex()
        .flex_none()
        .h(px(DIFF_LINE_HEIGHT))
        .whitespace_nowrap()
        .overflow_hidden()
        .text_color(rgb(foreground))
        .when_some(background, |row, background| row.bg(background))
        .when(selected, |row| row.bg(rgba(glass::SELECTION)))
        .child(div().flex_none().w(px(16.0)).text_center().child(prefix))
        .child(text);
    clicks(row, line, activate, cx).into_any_element()
}
