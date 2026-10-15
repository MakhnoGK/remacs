use std::ops::Range;

use gpui::{
    AnyElement, Context, Div, FontWeight, HighlightStyle, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Pixels, ScrollHandle, StyledText, div, linear_color_stop, linear_gradient,
    prelude::*, px, rgb, rgba, svg,
};
use remacs_core::{
    BannerPart, Candidate, CommitChain, Count, EntryAction, Expansion, Icon, Minibuffer,
    PromptOption, RefKind, Tone, parse_sequence,
};
use remacs_theme::{color, font, glass};

use crate::assets::CHEVRON_DOWN_ICON;
use crate::dialog_view::render_dialog;
use crate::display_line::DisplayLine;
use crate::editor_view::EditorView;
use crate::outline_view::ref_chip;
use crate::widgets::{
    GlassBlur, block_cursor, choice_chip, glass_surface, hsla, icon, kbd, list_items,
    modal_surface, modified_dot, option_chip, segment, segmented, shadow, text_with_cursor,
    tone_color,
};

const POPOVER_TOP: f32 = 60.0;
/// The problem list opens from the status pill, above it.
const STATUS_POPOVER_RIGHT: f32 = 16.0;
const STATUS_POPOVER_BOTTOM: f32 = 56.0;
const PROBLEM_CATEGORY: &str = "problem";
const SEVERITY_ICONS: [Icon; 3] = [Icon::Error, Icon::Warning, Icon::Info];
const POSITION_WIDTH: f32 = 44.0;
const POPOVER_WIDTH: f32 = 680.0;
/// Opened from the toolbar, under its right end.
const TOOLBAR_POPOVER_WIDTH: f32 = 500.0;
const TOOLBAR_POPOVER_TOP: f32 = 44.0;
const TOOLBAR_POPOVER_RIGHT: f32 = 14.0;
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
/// Of a message's description, shown at once.
const DESCRIPTION_LINES: usize = 4;
const DESCRIPTION_LINE_HEIGHT: f32 = 22.0;

/// Dims the editor behind an open prompt; a modal one also blurs it a little.
pub fn render_scrim(modal: bool) -> impl IntoElement {
    let scrim = div().absolute().inset_0();
    if modal {
        scrim
            .bg(rgba(glass::MODAL_SCRIM))
            .glass_blur(glass::MODAL_SCRIM_BLUR, 1.0)
    } else {
        scrim.bg(rgba(glass::SCRIM))
    }
}

/// The problem list: rows of messages with their positions, anchored to the status pill.
pub fn is_problem_list(minibuffer: &Minibuffer) -> bool {
    minibuffer.prompt().category == PROBLEM_CATEGORY
}

/// A floating panel, like the prompt's.
pub fn popover(width: f32) -> Div {
    panel_on(glass_surface(glass::SURFACE_RADIUS), width)
}

/// A panel for a question that must be answered.
pub fn modal(width: f32) -> Div {
    panel_on(modal_surface(), width)
}

fn panel_on(surface: Div, width: f32) -> Div {
    surface
        .w(px(width))
        .p(px(8.0))
        .font_family(font::UI)
        .text_color(rgb(color::FG_MAIN))
}

/// Whether the prompt shows the editable input, which dialogs, titled prompts and
/// questions leave out.
pub fn draws_input(minibuffer: &Minibuffer) -> bool {
    minibuffer.dialog().is_none() && minibuffer.prompt().title.is_none() && !minibuffer.is_question()
}

/// `input_scroll` keeps the input's sideways scroll between frames; it keeps
/// `input_margin` in view on each side of the cursor.
pub fn render_minibuffer(
    minibuffer: &Minibuffer,
    input_scroll: &ScrollHandle,
    input_margin: Pixels,
    cx: &mut Context<EditorView>,
) -> AnyElement {
    if let Some(dialog) = minibuffer.dialog() {
        return centered(render_dialog(dialog, cx));
    }
    let prompt = minibuffer.prompt();
    let width = match () {
        _ if prompt.from_toolbar => TOOLBAR_POPOVER_WIDTH,
        _ if prompt.wide => WIDE_POPOVER_WIDTH,
        _ => POPOVER_WIDTH,
    };
    let has_switch = prompt.options.iter().any(PromptOption::is_switch);
    let has_chip = prompt.choices.iter().any(|choice| !choice.segmented)
        || prompt.options.iter().any(|option| !option.is_switch());
    let field = match &prompt.title {
        Some(title) => render_title(title).into_any_element(),
        None => render_field(minibuffer, input_scroll, input_margin).into_any_element(),
    };
    let chips = render_chips(minibuffer, !has_switch, cx);
    let (chips_first, chips_last) = if prompt.chips_first {
        (chips, None)
    } else {
        (None, chips)
    };
    let popover = popover(width)
        .when(
            has_switch || (!has_chip && !minibuffer.subject().is_empty()),
            |popover| popover.child(render_switches(minibuffer, cx)),
        )
        .child(field)
        .when(!prompt.summary.is_empty(), |popover| {
            popover.child(render_summary(&prompt.summary))
        })
        .children(chips_first)
        .children(render_segmented_choices(minibuffer, cx))
        .children(render_description(minibuffer))
        .when(!minibuffer.candidates().is_empty(), |popover| {
            popover.child(render_candidates(minibuffer, cx))
        })
        .when_some(
            minibuffer.completions().outcome.clone(),
            |popover, outcome| popover.child(render_outcome(outcome)),
        )
        .when(!prompt.items.is_empty(), |popover| {
            popover
                .child(render_group_title(
                    prompt.items_title,
                    Some(prompt.items_summary.clone()),
                    false,
                ))
                .children(list_items(prompt.items.clone(), MAX_LISTED_ITEMS, 30.0))
        })
        .children(chips_last)
        .child(render_hints(minibuffer.hints()));
    if is_problem_list(minibuffer) {
        return div()
            .absolute()
            .right(px(STATUS_POPOVER_RIGHT))
            .bottom(px(STATUS_POPOVER_BOTTOM))
            .child(popover)
            .into_any_element();
    }
    if prompt.from_toolbar {
        return div()
            .absolute()
            .top(px(TOOLBAR_POPOVER_TOP))
            .right(px(TOOLBAR_POPOVER_RIGHT))
            .child(popover)
            .into_any_element();
    }
    centered(popover)
}

/// In place of the field: what the prompt does, and a note on the right.
fn render_title((title, note): &(String, String)) -> Div {
    div()
        .flex()
        .items_baseline()
        .justify_between()
        .gap(px(12.0))
        .pt(px(6.0))
        .px(px(10.0))
        .pb(px(8.0))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(font::UI_SIZE))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.clone()),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_DIM))
                .child(note.clone()),
        )
}

/// Chips and words under the title, e.g. how far two branches are apart.
fn render_summary(parts: &[BannerPart]) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0))
        .px(px(10.0))
        .pb(px(10.0))
        .text_size(px(font::STATUS_SIZE))
        .text_color(rgb(color::FG_DIM))
        .children(parts.iter().map(|part| match part {
            BannerPart::Chip(chip) => ref_chip(chip.clone()),
            BannerPart::Word(word) => div().child(word.clone()),
        }))
}

/// Choices drawn as segments, every value shown, the chosen one lit; a click picks the
/// next.
fn render_segmented_choices(minibuffer: &Minibuffer, cx: &mut Context<EditorView>) -> Option<Div> {
    let choices: Vec<Div> = minibuffer
        .prompt()
        .choices
        .iter()
        .enumerate()
        .filter(|(_, choice)| choice.segmented)
        .map(|(index, choice)| {
            let selected = choice.selected.get();
            let segments = choice
                .values
                .iter()
                .enumerate()
                .map(|(position, value)| segment(value, position == selected));
            segmented().children(segments).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    view.next_prompt_choice(index, cx);
                }),
            )
        })
        .collect();
    (!choices.is_empty()).then(|| div().flex().gap(px(8.0)).px(px(10.0)).children(choices))
}

/// Switches between two settings, then what the prompt acts on, above the input.
fn render_switches(minibuffer: &Minibuffer, cx: &mut Context<EditorView>) -> Div {
    let prompt = minibuffer.prompt();
    let switches = prompt
        .options
        .iter()
        .enumerate()
        .filter(|(_, option)| option.is_switch() && option.is_shown())
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
                .child(minibuffer.subject().to_owned()),
        )
}

/// Choices, then settings that are on or off, under the lists; then the completions'
/// footnote, or what the prompt acts on if `with_subject`.
fn render_chips(
    minibuffer: &Minibuffer,
    with_subject: bool,
    cx: &mut Context<EditorView>,
) -> Option<Div> {
    let prompt = minibuffer.prompt();
    let footnote = minibuffer.completions().footnote.clone();
    let mut chips: Vec<Div> = prompt
        .choices
        .iter()
        .enumerate()
        .filter(|(_, choice)| !choice.segmented)
        .map(|(index, choice)| {
            choice_chip(choice.label, choice.value().unwrap_or_default()).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    view.next_prompt_choice(index, cx);
                }),
            )
        })
        .collect();
    let options = prompt
        .options
        .iter()
        .enumerate()
        .filter(|(_, option)| !option.is_switch() && option.is_shown())
        .map(|(index, option)| {
            let chip = option_chip(
                &option.labels[0],
                &format!("M-{}", option.key),
                option.is_on(),
            );
            option_click(chip, index, cx)
        });
    chips.extend(options);
    (!chips.is_empty()).then(|| {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.0))
            .pt(px(12.0))
            .px(px(10.0))
            .pb(px(2.0))
            .children(chips)
            .when_some(footnote, |chips, footnote| {
                chips.child(div().flex_1()).child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(6.0))
                        .text_size(px(font::STATUS_SIZE))
                        .text_color(rgb(color::FG_DIM))
                        .child(
                            div()
                                .size(px(9.0))
                                .rounded_full()
                                .border(px(1.6))
                                .border_color(rgb(color::INFO)),
                        )
                        .child(footnote),
                )
            })
            .when(with_subject && !minibuffer.subject().is_empty(), |chips| {
                chips.child(div().flex_1()).child(
                    div()
                        .flex_none()
                        .text_size(px(font::STATUS_SIZE))
                        .text_color(rgb(color::FG_FAINT))
                        .child(minibuffer.subject().to_owned()),
                )
            })
    })
}

/// What choosing would make: a summary, then the base, a dot per commit and the tip.
fn render_outcome((summary, chain): (String, CommitChain)) -> impl IntoElement {
    let dot = |color_hex: u32| {
        div()
            .flex_none()
            .size(px(10.0))
            .rounded_full()
            .bg(rgb(color_hex))
            .border_2()
            .border_color(rgb(color::BG_POPUP))
    };
    let line = |from: u32, to: u32| {
        div().w(px(36.0)).h(px(2.0)).bg(linear_gradient(
            90.0,
            linear_color_stop(hsla(from), 0.0),
            linear_color_stop(hsla(to), 1.0),
        ))
    };
    // A remote's commit, like a merge base, is grey; what comes in is the base's color.
    let base_color = match chain.base.kind {
        RefKind::Remote => color::FG_LINE_NUMBER,
        _ => color::graph(1),
    };
    let (incoming_color, tip_color) = (color::graph(1), color::graph(0));
    let mut chain_row = div()
        .flex()
        .items_center()
        .py(px(6.0))
        .px(px(10.0))
        .child(ref_chip(chain.base).mr(px(12.0)))
        .child(dot(base_color));
    let mut previous = base_color;
    let colors = std::iter::repeat_n(incoming_color, chain.incoming)
        .chain(std::iter::repeat_n(tip_color, chain.commits));
    for dot_color in colors {
        chain_row = chain_row
            .child(line(previous, dot_color))
            .child(dot(dot_color));
        previous = dot_color;
    }
    let summary = Some(summary).filter(|summary| !summary.is_empty());
    div()
        .child(render_group_title("Result", summary, false))
        .child(chain_row.child(ref_chip(chain.tip).ml(px(12.0))))
        .children(chain.note.map(|note| {
            div()
                .px(px(10.0))
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_DIM))
                .child(note)
        }))
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

fn render_field(
    minibuffer: &Minibuffer,
    input_scroll: &ScrollHandle,
    input_margin: Pixels,
) -> impl IntoElement {
    let prompt = minibuffer.prompt();
    let field = div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .h(px(FIELD_HEIGHT))
        .px(px(12.0))
        .bg(rgba(glass::FIELD))
        .rounded(px(8.0))
        .border_1()
        .border_color(field_border(
            !minibuffer.in_description() && !minibuffer.in_list(),
        ));
    if minibuffer.is_question() {
        return field
            .text_size(px(font::INPUT_SIZE))
            .child(prompt.label.clone());
    }
    let typed_font = |typed: bool| if typed { font::mono() } else { font::UI.into() };
    field
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .h(px(24.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .bg(rgba(glass::CHIP))
                .font_family(typed_font(parse_sequence(minibuffer.label()).is_some()))
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_ALT))
                .child(minibuffer.label().to_owned()),
        )
        .when(!prompt.prefix.is_empty(), |field| {
            field.child(
                div()
                    .flex_none()
                    .font_family(font::mono())
                    .text_size(px(font::INPUT_SIZE))
                    .text_color(rgb(color::FG_DIM))
                    .child(prompt.prefix.clone()),
            )
        })
        .child(render_input(minibuffer, input_scroll, input_margin))
        .child(if is_problem_list(minibuffer) {
            render_severity_counts(minibuffer.candidates()).into_any_element()
        } else if let Some(message) = &prompt.message {
            let length = minibuffer.input_text().chars().count();
            div()
                .flex_none()
                .font_family(font::mono())
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(if length > message.subject_limit {
                    color::WARNING
                } else {
                    color::FG_DIM
                }))
                .child(format!("{length}/{}", message.subject_limit))
                .into_any_element()
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
                .bg(rgba(glass::CONTROL))
                .border_1()
                .border_color(rgba(0xffffff12))
                .font_family(font::mono())
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_KBD))
                .child(icon(severity, color::FG_KBD))
                .child(count(severity).to_string())
        }))
}

/// Scrolled sideways just enough to keep the cursor and `margin` on each side of it in
/// view, like Emacs `hscroll`: the text before it, the cursor and the text after it are
/// separate children so the view can scroll to the cursor's.
fn render_input(
    minibuffer: &Minibuffer,
    scroll: &ScrollHandle,
    margin: Pixels,
) -> impl IntoElement {
    let input = minibuffer.input();
    let mut line = DisplayLine::new(&minibuffer.input_text());
    let cursor = line.cursor_cell(input.point_column());
    let part = |range: Range<usize>| div().flex_none().child(line.text[range].to_owned());
    let cursor_part = if minibuffer.in_description() || !minibuffer.is_typed() {
        div().flex_none()
    } else {
        block_cursor()
    };
    // Padding the view scrolls into sight with the cursor, taken back by as much margin so
    // the text does not move; none past the end, where the text before fills the field.
    let text_after = cursor.end < line.text.len();
    let cursor_with_margin = div()
        .flex_none()
        .pl(margin)
        .ml(-margin)
        .when(text_after, |part| part.pr(margin).mr(-margin))
        .child(cursor_part.child(line.text[cursor.clone()].to_owned()));
    scroll.scroll_to_item(CURSOR_CHILD);
    div()
        .id("minibuffer-input")
        .flex()
        .flex_1()
        .min_w_0()
        .overflow_x_scroll()
        .track_scroll(scroll)
        .whitespace_nowrap()
        .font_family(font::mono())
        .text_size(px(font::INPUT_SIZE))
        .child(part(0..cursor.start))
        .child(cursor_with_margin)
        .child(part(cursor.end..line.text.len()))
        .when(input.text().is_empty(), |field| {
            field.child(
                div()
                    .text_color(rgb(color::FG_PLACEHOLDER))
                    .child(minibuffer.prompt().placeholder),
            )
        })
}

/// Lit while it is the one being typed in.
fn field_border(active: bool) -> gpui::Rgba {
    rgba(if active { 0x2fafff59 } else { 0xffffff0f })
}

/// A message's description under its subject: a few lines, scrolled to the cursor.
fn render_description(minibuffer: &Minibuffer) -> Option<impl IntoElement> {
    let description = minibuffer.description()?;
    let active = minibuffer.in_description();
    let text = description.contents(0..description.text().len());
    let point_line = description.point_line();
    let first = if active {
        point_line.saturating_sub(DESCRIPTION_LINES - 1)
    } else {
        0
    };
    let lines = text
        .split('\n')
        .enumerate()
        .skip(first)
        .take(DESCRIPTION_LINES)
        .map(|(index, source)| {
            let mut line = DisplayLine::new(source);
            let text = if active && index == point_line {
                let cursor = line.cursor_cell(description.point_column());
                text_with_cursor(&line.text, &Vec::new(), cursor)
            } else {
                div().child(line.text)
            };
            div()
                .h(px(DESCRIPTION_LINE_HEIGHT))
                .whitespace_nowrap()
                .overflow_hidden()
                .child(text)
        });
    let placeholder = minibuffer
        .prompt()
        .message
        .as_ref()
        .map_or("", |message| message.placeholder);
    let label = render_group_title("Description", None, false);
    let field = div()
        .flex()
        .flex_col()
        .h(px(DESCRIPTION_LINE_HEIGHT * DESCRIPTION_LINES as f32 + 16.0))
        .py(px(8.0))
        .px(px(12.0))
        .bg(rgba(glass::FIELD))
        .rounded(px(8.0))
        .border_1()
        .border_color(field_border(active))
        .font_family(font::mono())
        .text_size(px(font::MONO_SIZE))
        .line_height(px(DESCRIPTION_LINE_HEIGHT))
        .text_color(rgb(color::FG_BODY))
        .children(lines)
        .when(text.is_empty() && !active, |field| {
            field.child(
                div()
                    .text_color(rgb(color::FG_PLACEHOLDER))
                    .child(placeholder),
            )
        });
    Some(div().child(label).child(field))
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
            let counted = is_problem_list || minibuffer.completions().count_groups;
            let count = counted.then(|| {
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
        } else if let Some(expansion) = &candidate.expansion {
            render_entry(
                candidate,
                expansion,
                index,
                selected,
                minibuffer.in_list(),
                cx,
            )
        } else if let Some(action) = candidate.action {
            render_step(candidate, action, index, selected, cx)
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
            row.bg(rgba(glass::SELECTION))
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
                .font_family(font::mono())
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(secondary))
                .child(candidate.annotation.clone()),
        )
        .into_any_element()
}

/// A step of a plan: the action on it as a button, its id and what it is.
fn render_step(
    candidate: &Candidate,
    (action, tone): (&'static str, Tone),
    index: usize,
    selected: bool,
    cx: &mut Context<EditorView>,
) -> AnyElement {
    let action_color = match tone {
        Tone::Plain => color::FG_BODY,
        tone => tone_color(tone, color::FG_BODY),
    };
    candidate_row(index, selected, cx)
        .gap(px(12.0))
        .h(px(WIDE_ROW_HEIGHT))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_between()
                .w(px(88.0))
                .h(px(24.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .bg(rgba(glass::CONTROL))
                .border_1()
                .border_color(rgba(0xffffff14))
                .font_family(font::mono())
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(action_color))
                .child(action)
                .child(
                    svg()
                        .path(CHEVRON_DOWN_ICON)
                        .size(px(10.0))
                        .text_color(rgb(action_color)),
                ),
        )
        .child(
            div()
                .flex_none()
                .w(px(64.0))
                .font_family(font::mono())
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(if selected {
                    color::FG_ALT
                } else {
                    color::FG_LINE_NUMBER
                }))
                .child(candidate.label.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(font::UI_SIZE))
                .child(candidate.annotation.clone()),
        )
        .into_any_element()
}

/// An entry: its id, what it is and when; selected, it opens up to show its items and
/// the buttons that act on it, lit while keys go to the list. A click selects it.
fn render_entry(
    candidate: &Candidate,
    expansion: &Expansion,
    index: usize,
    selected: bool,
    in_list: bool,
    cx: &mut Context<EditorView>,
) -> AnyElement {
    let header = |detail: String| {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .flex_none()
                    .px(px(6.0))
                    .py(px(2.0))
                    .rounded(px(5.0))
                    .bg(rgba(0xffffff0f))
                    .font_family(font::mono())
                    .text_size(px(font::STATUS_SIZE))
                    .text_color(rgb(color::FG_ALT))
                    .child(candidate.label.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(font::UI_SIZE))
                    .child(candidate.annotation.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(font::STATUS_SIZE))
                    .text_color(rgb(color::FG_FAINT))
                    .child(detail),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    view.focus_candidate(index, cx);
                }),
            )
    };
    if !selected {
        let detail = candidate.detail.clone().unwrap_or_default().into_owned();
        return header(detail)
            .h(px(WIDE_ROW_HEIGHT))
            .px(px(10.0))
            .rounded(px(8.0))
            .into_any_element();
    }
    let (border, background) = if in_list {
        (0x2fafff61, 0x2fafff14)
    } else {
        (0xffffff14, 0xffffff08)
    };
    let (destructive, actions): (Vec<EntryAction>, Vec<EntryAction>) = expansion
        .actions
        .iter()
        .partition(|action| action.destructive);
    let button = |action: EntryAction| {
        let command = action.command;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .h(px(28.0))
            .pl(px(10.0))
            .pr(px(5.0))
            .rounded(px(7.0))
            .bg(rgba(glass::CONTROL))
            .border_1()
            .border_color(rgba(if action.destructive {
                0xff5f594d
            } else {
                0xffffff14
            }))
            .shadow(vec![shadow(0x00000080, 1.0, 2.0, 0.0)])
            .text_size(px(font::STATUS_SIZE))
            .text_color(rgb(if action.destructive {
                color::FG_FLAGGED
            } else {
                color::FG_MAIN
            }))
            .child(action.label)
            .child(kbd(action.key, false).h(px(18.0)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    view.run_command(command, cx);
                }),
            )
    };
    let buttons = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .children(actions.into_iter().map(button))
        .child(div().flex_1())
        .children(destructive.into_iter().map(button));
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(8.0))
        .pt(px(8.0))
        .px(px(10.0))
        .pb(px(10.0))
        .rounded(px(10.0))
        .bg(rgba(background))
        .border_1()
        .border_color(rgba(border))
        .child(header(expansion.detail.clone()).h(px(28.0)))
        .when(!expansion.items.is_empty(), |entry| {
            entry.child(
                div()
                    .flex()
                    .flex_col()
                    .py(px(4.0))
                    .rounded(px(8.0))
                    .bg(rgba(glass::FIELD))
                    .children(list_items(expansion.items.clone(), MAX_LISTED_ITEMS, 26.0)),
            )
        })
        .child(buttons)
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
                        .font_family(font::mono())
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
        .font_family(font::mono())
        .text_size(px(font::MONO_SIZE))
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

#[cfg(test)]
mod tests {
    use remacs_core::{Editor, Prompt};

    use super::draws_input;

    /// Until the input is laid out, every frame asks for another; a question must not.
    #[test]
    fn only_prompts_with_an_input_wait_for_its_layout() {
        let mut asking = Editor::new(&[]);
        asking.ask_y_or_n("Proceed?", |_, _| Ok(())).unwrap();
        assert!(!draws_input(asking.minibuffer().unwrap()));

        let mut reading = Editor::new(&[]);
        reading
            .read(Prompt::default(), |_, _| Default::default(), |_, _| Ok(()))
            .unwrap();
        assert!(draws_input(reading.minibuffer().unwrap()));
    }
}
