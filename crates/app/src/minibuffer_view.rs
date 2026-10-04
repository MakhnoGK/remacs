use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StyledText, div, prelude::*, px, rgb, rgba, svg,
};
use remacs_core::{Candidate, Count, Icon, Minibuffer, parse_sequence};
use remacs_theme::{color, font};

use crate::assets::{FILE_ICON, FOLDER_ICON};
use crate::display_line::DisplayLine;
use crate::editor_view::EditorView;
use crate::widgets::{cursor_style, hsla, kbd, modified_dot, shadow};

const POPOVER_TOP: f32 = 60.0;
const POPOVER_WIDTH: f32 = 680.0;
const WIDE_POPOVER_WIDTH: f32 = 760.0;
const FIELD_HEIGHT: f32 = 44.0;
const ROW_HEIGHT: f32 = 34.0;
const WIDE_ROW_HEIGHT: f32 = 36.0;
/// M-x lines command names up in a column, with their documentation beside them.
const COMMAND_CATEGORY: &str = "command";
const COMMAND_NAME_WIDTH: f32 = 232.0;

/// Dims the editor behind an open prompt.
pub fn render_scrim() -> impl IntoElement {
    div().absolute().inset_0().bg(rgba(0x0000006b))
}

pub fn render_minibuffer(minibuffer: &Minibuffer, cx: &mut Context<EditorView>) -> AnyElement {
    let prompt = minibuffer.prompt();
    let width = if prompt.wide {
        WIDE_POPOVER_WIDTH
    } else {
        POPOVER_WIDTH
    };
    let popover = div()
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
        .child(render_field(minibuffer))
        .when(!minibuffer.candidates().is_empty(), |popover| {
            popover.child(render_candidates(minibuffer, cx))
        })
        .child(render_hints(prompt.hints));
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

fn render_field(minibuffer: &Minibuffer) -> impl IntoElement {
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
        .child(render_input(minibuffer))
        .child(
            div()
                .flex_none()
                .font_family(typed_font(prompt.count == Count::Position))
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_DIM))
                .child(count_text(minibuffer)),
        )
}

fn render_input(minibuffer: &Minibuffer) -> impl IntoElement {
    let input = minibuffer.input();
    let mut line = DisplayLine::new(&minibuffer.input_text());
    let cursor = line.cursor_cell(input.point_column());
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .font_family(font::MONO)
        .text_size(px(font::INPUT_SIZE))
        .child(StyledText::new(line.text).with_highlights([(cursor, cursor_style())]))
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
    let mut list = div().flex().flex_col().gap(px(2.0));
    let mut group = None;
    for index in minibuffer.visible() {
        let candidate = &candidates[index];
        if let Some(title) = candidate.group.filter(|_| candidate.group != group) {
            list = list.child(render_group_title(title));
        }
        group = candidate.group;
        let selected = minibuffer.selected() == Some(index);
        list = list.child(render_candidate(minibuffer, candidate, index, selected, cx));
    }
    list.when(group.is_none(), |list| list.pt(px(6.0)))
}

fn render_group_title(title: &str) -> impl IntoElement {
    div()
        .pt(px(12.0))
        .px(px(10.0))
        .pb(px(4.0))
        .text_size(px(font::SMALL_SIZE))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(color::FG_FAINT))
        .child(title.to_uppercase())
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
    let secondary = if selected {
        color::FG_ALT
    } else {
        color::FG_DIM
    };
    let match_style = HighlightStyle {
        color: Some(hsla(if selected {
            color::ACCENT_ON_SELECTION
        } else {
            color::ACCENT
        })),
        ..Default::default()
    };
    let label = StyledText::new(candidate.label.clone()).with_highlights(
        candidate
            .matches
            .iter()
            .map(|range| (range.clone(), match_style)),
    );
    let name = div()
        .flex_none()
        .overflow_hidden()
        .whitespace_nowrap()
        .font_family(font::MONO)
        .text_size(px(font::CODE_SIZE))
        .text_color(rgb(if candidate.dim {
            color::FG_DIM
        } else {
            color::FG_MAIN
        }))
        .when(is_command, |name| name.w(px(COMMAND_NAME_WIDTH)))
        .child(label);

    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(if is_command { 14.0 } else { 10.0 }))
        .h(px(if prompt.wide {
            WIDE_ROW_HEIGHT
        } else {
            ROW_HEIGHT
        }))
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
        .when_some(candidate.icon, |row, icon| {
            let path = match icon {
                Icon::File => FILE_ICON,
                Icon::Directory => FOLDER_ICON,
            };
            row.child(
                svg()
                    .path(path)
                    .flex_none()
                    .size(px(16.0))
                    .text_color(rgb(secondary)),
            )
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
        .when_some(candidate.detail, |row, detail| {
            row.child(
                div()
                    .flex_none()
                    .text_size(px(font::STATUS_SIZE))
                    .text_color(rgb(secondary))
                    .child(detail),
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
