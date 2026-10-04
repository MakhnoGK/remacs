use std::ops::Range;

use gpui::{
    AnyElement, Context, Div, FontWeight, HighlightStyle, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, ScrollHandle, StyledText, div, prelude::*, px, rgb, rgba,
};
use remacs_core::{Candidate, Count, Icon, Minibuffer, Prompt, PromptOption, parse_sequence};
use remacs_theme::{color, font};

use crate::dialog_view::render_dialog;
use crate::display_line::DisplayLine;
use crate::editor_view::EditorView;
use crate::widgets::{
    cursor_style, hsla, icon, kbd, list_items, modified_dot, option_chip, segment, segmented,
    shadow,
};

const POPOVER_TOP: f32 = 60.0;
/// The problem list opens from the status pill, above it.
const STATUS_POPOVER_RIGHT: f32 = 16.0;
const STATUS_POPOVER_BOTTOM: f32 = 56.0;
const PROBLEM_CATEGORY: &str = "problem";
const SEVERITY_ICONS: [Icon; 3] = [Icon::Error, Icon::Warning, Icon::Info];
const POSITION_WIDTH: f32 = 44.0;
const POPOVER_WIDTH: f32 = 680.0;
const WIDE_POPOVER_WIDTH: f32 = 760.0;
const FIELD_HEIGHT: f32 = 44.0;
const ROW_HEIGHT: f32 = 34.0;
const WIDE_ROW_HEIGHT: f32 = 36.0;
/// M-x lines command names up in a column, with their documentation beside them.
const COMMAND_CATEGORY: &str = "command";
const COMMAND_NAME_WIDTH: f32 = 232.0;
/// The input's children are the text before the cursor, the cursor and the rest.
const CURSOR_CHILD: usize = 1;
/// Items listed for reference give way to the candidates beyond this many.
const MAX_LISTED_ITEMS: usize = 5;

/// Dims the editor behind an open prompt.
pub fn render_scrim() -> impl IntoElement {
    div().absolute().inset_0().bg(rgba(0x0000006b))
}

/// The problem list: rows of messages with their positions, anchored to the status pill.
pub fn is_problem_list(minibuffer: &Minibuffer) -> bool {
    minibuffer.prompt().category == PROBLEM_CATEGORY
}

/// A floating panel, like the prompt's.
pub fn popover(width: f32) -> Div {
    div()
        .w(px(width))
        .p(px(8.0))
        .bg(rgb(color::BG_POPUP))
        .border_1()
        .border_color(rgba(0xffffff14))
        .rounded(px(12.0))
        .shadow(vec![
            shadow(0x000000b3, 1.0, 2.0, 0.0),
            shadow(0x00000099, 16.0, 40.0, 0.0),
        ])
        .font_family(font::UI)
        .text_color(rgb(color::FG_MAIN))
}

/// `input_scroll` keeps the input's sideways scroll between frames.
pub fn render_minibuffer(
    minibuffer: &Minibuffer,
    input_scroll: &ScrollHandle,
    cx: &mut Context<EditorView>,
) -> AnyElement {
    if let Some(dialog) = minibuffer.dialog() {
        return centered(render_dialog(dialog, cx));
    }
    let prompt = minibuffer.prompt();
    let width = if prompt.wide {
        WIDE_POPOVER_WIDTH
    } else {
        POPOVER_WIDTH
    };
    let has_switch = prompt.options.iter().any(PromptOption::is_switch);
    let popover = popover(width)
        .when(has_switch || !prompt.subject.is_empty(), |popover| {
            popover.child(render_switches(prompt, cx))
        })
        .child(render_field(minibuffer, input_scroll))
        .when(!minibuffer.candidates().is_empty(), |popover| {
            popover.child(render_candidates(minibuffer, cx))
        })
        .when(!prompt.items.is_empty(), |popover| {
            popover
                .child(render_group_title(
                    prompt.items_title,
                    Some(prompt.items_summary.clone()),
                    false,
                ))
                .children(list_items(prompt.items.clone(), MAX_LISTED_ITEMS, 30.0))
        })
        .children(render_chips(prompt, cx))
        .child(render_hints(minibuffer.hints()));
    if is_problem_list(minibuffer) {
        return div()
            .absolute()
            .right(px(STATUS_POPOVER_RIGHT))
            .bottom(px(STATUS_POPOVER_BOTTOM))
            .child(popover)
            .into_any_element();
    }
    centered(popover)
}

/// Switches between two settings, then what the prompt acts on, above the input.
fn render_switches(prompt: &Prompt, cx: &mut Context<EditorView>) -> Div {
    let switches = prompt
        .options
        .iter()
        .enumerate()
        .filter(|(_, option)| option.is_switch())
        .map(|(index, option)| {
            let segments = option.labels.iter().enumerate().map(|(position, label)| {
                let selected = (position == 1) == option.is_on();
                option_click(segment(label, selected), index, cx)
            });
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.0))
                .child(segmented().children(segments))
                .child(kbd(&format!("M-{}", option.key), false))
        });
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .pt(px(2.0))
        .px(px(2.0))
        .pb(px(10.0))
        .children(switches)
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(font::UI_SIZE))
                .text_color(rgb(color::FG_BODY))
                .child(prompt.subject.clone()),
        )
}

/// Settings that are on or off, under the lists.
fn render_chips(prompt: &Prompt, cx: &mut Context<EditorView>) -> Option<Div> {
    let chips: Vec<Div> = prompt
        .options
        .iter()
        .enumerate()
        .filter(|(_, option)| !option.is_switch())
        .map(|(index, option)| {
            let chip = option_chip(
                option.labels[0],
                &format!("M-{}", option.key),
                option.is_on(),
            );
            option_click(chip, index, cx)
        })
        .collect();
    (!chips.is_empty()).then(|| {
        div()
            .flex()
            .flex_wrap()
            .gap(px(6.0))
            .pt(px(12.0))
            .px(px(10.0))
            .pb(px(2.0))
            .children(chips)
    })
}

fn option_click(element: Div, index: usize, cx: &mut Context<EditorView>) -> Div {
    element.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |view, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            view.toggle_prompt_option(index, cx);
        }),
    )
}

/// Near the top of the window, centered across it.
fn centered(popover: impl IntoElement) -> AnyElement {
    div()
        .absolute()
        .top(px(POPOVER_TOP))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(popover)
        .into_any_element()
}

fn render_field(minibuffer: &Minibuffer, input_scroll: &ScrollHandle) -> impl IntoElement {
    let prompt = minibuffer.prompt();
    let field = div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .h(px(FIELD_HEIGHT))
        .px(px(12.0))
        .bg(rgb(color::BG_FIELD))
        .rounded(px(8.0))
        .border_1()
        .border_color(rgba(0x2fafff59));
    if minibuffer.is_question() {
        return field
            .text_size(px(font::INPUT_SIZE))
            .child(prompt.label.clone());
    }
    let typed_font = |typed: bool| if typed { font::MONO } else { font::UI };
    field
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .h(px(24.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .bg(rgb(color::BG_CHIP))
                .font_family(typed_font(parse_sequence(&prompt.label).is_some()))
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_ALT))
                .child(prompt.label.clone()),
        )
        .when(!prompt.prefix.is_empty(), |field| {
            field.child(
                div()
                    .flex_none()
                    .font_family(font::MONO)
                    .text_size(px(font::INPUT_SIZE))
                    .text_color(rgb(color::FG_DIM))
                    .child(prompt.prefix.clone()),
            )
        })
        .child(render_input(minibuffer, input_scroll))
        .child(if is_problem_list(minibuffer) {
            render_severity_counts(minibuffer.candidates()).into_any_element()
        } else {
            div()
                .flex_none()
                .font_family(typed_font(prompt.count == Count::Position))
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_DIM))
                .child(match minibuffer.note() {
                    Some(note) => note.to_owned(),
                    None => count_text(minibuffer),
                })
                .into_any_element()
        })
}

/// How many of the listed problems are errors, warnings and notes.
fn render_severity_counts(candidates: &[Candidate]) -> impl IntoElement {
    let count = |severity| {
        candidates
            .iter()
            .filter(|candidate| candidate.icon == Some(severity))
            .count()
    };
    div()
        .flex()
        .flex_none()
        .gap(px(4.0))
        .children(SEVERITY_ICONS.map(|severity| {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(26.0))
                .px(px(9.0))
                .rounded(px(6.0))
                .bg(rgb(color::BG_BUTTON))
                .border_1()
                .border_color(rgba(0xffffff12))
                .font_family(font::MONO)
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_KBD))
                .child(icon(severity, color::FG_KBD))
                .child(count(severity).to_string())
        }))
}

/// Scrolled sideways just enough to keep the cursor in view, like Emacs `hscroll`: the
/// text before it, the cursor and the text after it are separate children so the view
/// can scroll to the cursor's.
fn render_input(minibuffer: &Minibuffer, scroll: &ScrollHandle) -> impl IntoElement {
    let input = minibuffer.input();
    let mut line = DisplayLine::new(&minibuffer.input_text());
    let cursor = line.cursor_cell(input.point_column());
    let part = |range: Range<usize>| div().flex_none().child(line.text[range].to_owned());
    let cursor_text = line.text[cursor.clone()].to_owned();
    scroll.scroll_to_item(CURSOR_CHILD);
    div()
        .id("minibuffer-input")
        .flex()
        .flex_1()
        .min_w_0()
        .overflow_x_scroll()
        .track_scroll(scroll)
        .whitespace_nowrap()
        .font_family(font::MONO)
        .text_size(px(font::INPUT_SIZE))
        .child(part(0..cursor.start))
        .child(
            div().flex_none().child(
                StyledText::new(cursor_text.clone())
                    .with_highlights([(0..cursor_text.len(), cursor_style())]),
            ),
        )
        .child(part(cursor.end..line.text.len()))
        .when(input.text().is_empty(), |field| {
            field.child(
                div()
                    .text_color(rgb(color::FG_PLACEHOLDER))
                    .child(minibuffer.prompt().placeholder),
            )
        })
}

fn count_text(minibuffer: &Minibuffer) -> String {
    let total = minibuffer.total();
    match minibuffer.prompt().count {
        Count::Position => format!(
            "{}/{total}",
            minibuffer.selected().map_or(0, |index| index + 1)
        ),
        Count::Matches if total == 1 => "1 match".into(),
        Count::Matches => format!("{total} matches"),
        Count::Total => total.to_string(),
    }
}

fn render_candidates(minibuffer: &Minibuffer, cx: &mut Context<EditorView>) -> impl IntoElement {
    let candidates = minibuffer.candidates();
    let is_problem_list = is_problem_list(minibuffer);
    let mut list = div().flex().flex_col().gap(px(2.0));
    let mut group = None;
    for index in minibuffer.visible() {
        let candidate = &candidates[index];
        let candidate_group = candidate.group.as_deref();
        if let Some(title) = candidate_group.filter(|_| candidate_group != group) {
            // Problems are grouped by file: names keep their case and show their count.
            let count = is_problem_list.then(|| {
                candidates
                    .iter()
                    .filter(|other| other.group.as_deref() == Some(title))
                    .count()
            });
            list = list.child(render_group_title(
                title,
                count.map(|count| count.to_string()),
                is_problem_list,
            ));
        }
        group = candidate_group;
        let selected = minibuffer.selected() == Some(index);
        let row = if is_problem_list {
            render_problem(candidate, index, selected, cx)
        } else {
            render_candidate(minibuffer, candidate, index, selected, cx)
        };
        list = list.child(row);
    }
    list.when(group.is_none(), |list| list.pt(px(6.0)))
}

/// Upper case, unless it is a name; `summary` goes on the right.
fn render_group_title(title: &str, summary: Option<String>, is_name: bool) -> impl IntoElement {
    let title = if is_name {
        title.to_owned()
    } else {
        title.to_uppercase()
    };
    div()
        .flex()
        .justify_between()
        .pt(px(12.0))
        .px(px(10.0))
        .pb(px(4.0))
        .text_size(px(font::SMALL_SIZE))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(color::FG_FAINT))
        .child(title)
        .children(summary)
}

/// A row that answers the prompt with candidate `index` when clicked.
fn candidate_row(index: usize, selected: bool, cx: &mut Context<EditorView>) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .px(px(10.0))
        .rounded(px(8.0))
        .when(selected, |row| {
            row.bg(rgb(color::BG_SELECTION))
                .shadow(vec![shadow(0x00000073, 1.0, 3.0, 0.0)])
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                view.choose_candidate(index, cx);
            }),
        )
}

pub(crate) fn secondary_color(selected: bool) -> u32 {
    if selected {
        color::FG_ALT
    } else {
        color::FG_DIM
    }
}

fn match_style(selected: bool) -> HighlightStyle {
    HighlightStyle {
        color: Some(hsla(if selected {
            color::ACCENT_ON_SELECTION
        } else {
            color::ACCENT
        })),
        ..Default::default()
    }
}

/// `matches` within `range` of a label, relative to the range's start.
pub(crate) fn matches_in(
    matches: &[Range<usize>],
    range: Range<usize>,
    selected: bool,
) -> Vec<(Range<usize>, HighlightStyle)> {
    matches
        .iter()
        .map(|matched| matched.start.max(range.start)..matched.end.min(range.end))
        .filter(|matched| !matched.is_empty())
        .map(|matched| {
            (
                matched.start - range.start..matched.end - range.start,
                match_style(selected),
            )
        })
        .collect()
}

/// A problem: severity icon, message, source and `line:column`.
fn render_problem(
    candidate: &Candidate,
    index: usize,
    selected: bool,
    cx: &mut Context<EditorView>,
) -> AnyElement {
    let secondary = secondary_color(selected);
    candidate_row(index, selected, cx)
        .gap(px(12.0))
        .h(px(WIDE_ROW_HEIGHT))
        .children(candidate.icon.map(|severity| icon(severity, secondary)))
        .child(render_message(candidate, selected))
        .when_some(candidate.detail.clone(), |row, source| {
            row.child(
                div()
                    .flex_none()
                    .text_size(px(font::STATUS_SIZE))
                    .text_color(rgb(secondary))
                    .child(source.into_owned()),
            )
        })
        .child(
            div()
                .flex_none()
                .w(px(POSITION_WIDTH))
                .text_right()
                .font_family(font::MONO)
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(secondary))
                .child(candidate.annotation.clone()),
        )
        .into_any_element()
}

/// A message whose `quoted` parts, as diagnostics write code, are set in the code font.
fn render_message(candidate: &Candidate, selected: bool) -> impl IntoElement {
    let (code_color, code_background) = if selected {
        (color::FG_MAIN, 0xffffff1f)
    } else {
        (color::FG_ALT, 0xffffff12)
    };
    let mut message = div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_size(px(font::UI_SIZE));
    let mut start = 0;
    for (index, part) in candidate.label.split('`').enumerate() {
        let range = start..start + part.len();
        start = range.end + 1;
        if part.is_empty() {
            continue;
        }
        let highlights = matches_in(&candidate.matches, range, selected);
        let text = StyledText::new(part.to_owned()).with_highlights(highlights);
        let is_code = index % 2 == 1;
        message = message.child(
            div()
                .flex_none()
                .when(is_code, |code| {
                    code.px(px(5.0))
                        .rounded(px(4.0))
                        .bg(rgba(code_background))
                        .font_family(font::MONO)
                        .text_size(px(font::STATUS_SIZE))
                        .text_color(rgb(code_color))
                })
                .child(text),
        );
    }
    message
}

fn render_candidate(
    minibuffer: &Minibuffer,
    candidate: &Candidate,
    index: usize,
    selected: bool,
    cx: &mut Context<EditorView>,
) -> AnyElement {
    let prompt = minibuffer.prompt();
    let is_command = prompt.category == COMMAND_CATEGORY;
    let secondary = secondary_color(selected);
    let label = StyledText::new(candidate.label.clone()).with_highlights(matches_in(
        &candidate.matches,
        0..candidate.label.len(),
        selected,
    ));
    // Gives way to nothing but its own length: a long path is cut short.
    let name = div()
        .flex_initial()
        .min_w_0()
        .truncate()
        .font_family(font::MONO)
        .text_size(px(font::CODE_SIZE))
        .text_color(rgb(if candidate.dim {
            color::FG_DIM
        } else {
            color::FG_MAIN
        }))
        .when(is_command, |name| name.w(px(COMMAND_NAME_WIDTH)))
        .child(label);

    candidate_row(index, selected, cx)
        .gap(px(if is_command { 14.0 } else { 10.0 }))
        .h(px(if prompt.wide {
            WIDE_ROW_HEIGHT
        } else {
            ROW_HEIGHT
        }))
        .children(candidate.icon.map(|file| icon(file, secondary)))
        .when_some(candidate.verb, |row, verb| {
            row.child(div().flex_none().text_size(px(font::UI_SIZE)).child(verb))
        })
        .child(name)
        .when(candidate.modified, |row| row.child(modified_dot()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(secondary))
                .child(candidate.annotation.clone()),
        )
        .when_some(candidate.tag, |row, tag| {
            row.child(render_tag(tag, selected))
        })
        .when_some(candidate.detail.clone(), |row, detail| {
            row.child(
                div()
                    .flex_none()
                    .text_size(px(font::STATUS_SIZE))
                    .text_color(rgb(secondary))
                    .child(detail.into_owned()),
            )
        })
        .when(!candidate.keys.is_empty(), |row| {
            row.child(
                div()
                    .flex()
                    .flex_none()
                    .gap(px(4.0))
                    .children(candidate.keys.iter().map(|key| kbd(key, selected))),
            )
        })
        .into_any_element()
}

fn render_tag(tag: &'static str, selected: bool) -> impl IntoElement {
    let (background, foreground) = if selected {
        (0xffffff1f, color::FG_MAIN)
    } else {
        (0x2fafff24, color::FG_ALT)
    };
    div()
        .flex_none()
        .px(px(7.0))
        .py(px(2.0))
        .rounded(px(5.0))
        .bg(rgba(background))
        .text_size(px(font::SMALL_SIZE))
        .text_color(rgb(foreground))
        .child(tag)
}

fn render_hints(hints: &'static [(&'static str, &'static str)]) -> impl IntoElement {
    div()
        .flex()
        .gap(px(18.0))
        .mt(px(6.0))
        .pt(px(10.0))
        .px(px(10.0))
        .pb(px(4.0))
        .border_t_1()
        .border_color(rgba(0xffffff0f))
        .text_size(px(font::STATUS_SIZE))
        .text_color(rgb(color::FG_DIM))
        .children(hints.iter().map(|&(keys, action)| {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .children(keys.split_whitespace().map(|key| kbd(key, false)))
                .child(action)
        }))
}
