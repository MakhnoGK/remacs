//! A plugin's pane under the text, e.g. a debugger's frames, variables and watches: a
//! toolbar, then columns of rows.

use gpui::{Context, Div, FontWeight, ParentElement, Styled, div, prelude::*, px, rgb, rgba};
use remacs_core::{Pane, PaneButton, PaneColumn, PaneRow, Tone};
use remacs_theme::{color, font, glass};

use crate::editor_view::EditorView;
use crate::table_view::clickable;
use crate::widgets::{kbd, key_caps, tone_color};

/// Where the pane's bottom is over the window's, clear of the status pill.
pub const PANE_BOTTOM: f32 = 56.0;
/// Between the text and the pane.
pub const PANE_GAP: f32 = 8.0;
const TOOLBAR_HEIGHT: f32 = 44.0;
const COLUMN_HEADER_HEIGHT: f32 = 30.0;
const ROW_HEIGHT: f32 = 22.0;
const INDENT: f32 = 14.0;

pub fn render_pane(pane: Pane, cx: &mut Context<EditorView>) -> Div {
    let rows_fit = ((pane.height - TOOLBAR_HEIGHT - COLUMN_HEADER_HEIGHT) / ROW_HEIGHT)
        .floor()
        .max(1.0) as usize;
    div()
        .absolute()
        .left(px(16.0))
        .right(px(16.0))
        .bottom(px(PANE_BOTTOM))
        .h(px(pane.height))
        .flex()
        .flex_col()
        .rounded(px(12.0))
        .bg(rgba(0xffffff0a))
        .border_1()
        .border_color(rgba(0xffffff1f))
        .overflow_hidden()
        .font_family(font::UI)
        .text_size(px(font::STATUS_SIZE))
        .child(render_toolbar(
            pane.title,
            pane.subtitle,
            pane.state,
            pane.buttons,
            cx,
        ))
        .child(
            div().flex().flex_1().min_h_0().children(
                pane.columns
                    .into_iter()
                    .map(|column| render_column(column, rows_fit, cx)),
            ),
        )
}

fn render_toolbar(
    title: String,
    subtitle: String,
    (state, tone): (String, Tone),
    buttons: Vec<PaneButton>,
    cx: &mut Context<EditorView>,
) -> Div {
    let state_color = tone_color(tone, color::FG_DIM);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .h(px(TOOLBAR_HEIGHT))
        .pl(px(14.0))
        .pr(px(10.0))
        .border_b_1()
        .border_color(rgba(0xffffff12))
        .whitespace_nowrap()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(font::UI_SIZE))
                .text_color(rgb(color::FG_MAIN))
                .child(title),
        )
        .child(
            div()
                .font_family(font::mono())
                .text_color(rgb(color::FG_DIM))
                .child(subtitle),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(20.0))
                .px(px(8.0))
                .ml(px(4.0))
                .rounded(px(6.0))
                .bg(rgba((state_color << 8) | 0x29))
                .font_family(font::mono())
                .text_size(px(font::SMALL_SIZE))
                .text_color(rgb(state_color))
                .child(div().size(px(7.0)).rounded_full().bg(rgb(state_color)))
                .child(state),
        )
        .child(div().flex_1())
        .children(buttons.into_iter().map(|button| render_button(button, cx)))
}

fn render_button(button: PaneButton, cx: &mut Context<EditorView>) -> Div {
    let element = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .h(px(28.0))
        .pl(px(8.0))
        .pr(px(6.0))
        .rounded(px(7.0))
        .bg(rgba(glass::CONTROL))
        .border_1()
        .border_color(rgba(glass::CONTROL_BORDER))
        .text_color(rgb(color::FG_BODY))
        .when(!button.enabled, |element| element.opacity(0.35))
        .child(button.label)
        .children(key_caps(button.keys));
    if button.enabled {
        clickable(element.cursor_pointer(), button.action, cx)
    } else {
        element
    }
}

fn render_column(column: PaneColumn, rows_fit: usize, cx: &mut Context<EditorView>) -> Div {
    let PaneColumn {
        title,
        keys,
        width,
        rows,
        selected,
        focused,
        empty,
        hints,
    } = column;
    let header = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .h(px(COLUMN_HEADER_HEIGHT))
        .px(px(14.0))
        .text_size(px(font::SMALL_SIZE))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(if focused {
            color::FG_ALT
        } else {
            color::FG_FAINT
        }))
        .child(title.to_uppercase())
        .children((!keys.is_empty()).then(|| kbd(keys, false)))
        .when(focused, |header| {
            header
                .child(div().flex_1())
                .children(hints.iter().map(|&(keys, label)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .font_weight(FontWeight::NORMAL)
                        .children(key_caps(keys))
                        .child(label)
                }))
        });
    // Keeps the selected row in view.
    let first = selected.map_or(0, |selected| (selected + 1).saturating_sub(rows_fit));
    let body = if rows.is_empty() {
        div().child(
            div()
                .m(px(14.0))
                .p(px(14.0))
                .rounded(px(8.0))
                .border_1()
                .border_dashed()
                .border_color(rgba(0xffffff1f))
                .line_height(px(18.0))
                .text_color(rgb(color::FG_FAINT))
                .child(empty),
        )
    } else {
        div().flex().flex_col().children(
            rows.into_iter()
                .enumerate()
                .skip(first)
                .take(rows_fit)
                .map(|(index, row)| render_row(row, focused && selected == Some(index), cx)),
        )
    };
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .overflow_hidden()
        .border_r_1()
        .border_color(rgba(0xffffff12))
        .map(|column| match width {
            Some(width) => column.flex_none().w(px(width)),
            None => column.flex_1(),
        })
        .child(header)
        .child(body)
}

fn render_row(row: PaneRow, selected: bool, cx: &mut Context<EditorView>) -> Div {
    let PaneRow {
        depth,
        open,
        name,
        kind,
        value,
        detail,
        tag,
        tone,
        current,
        action,
    } = row;
    let name_color = match tone {
        Tone::Faint => color::FG_PLACEHOLDER,
        _ if value.is_some() => color::FUNCTION,
        _ => color::FG_BODY,
    };
    let expander = match open {
        Some(true) => "▾",
        Some(false) => "▸",
        None => "",
    };
    let element = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .h(px(ROW_HEIGHT))
        .mx(px(6.0))
        .pl(px(4.0 + INDENT * depth as f32))
        .pr(px(8.0))
        .rounded(px(6.0))
        .whitespace_nowrap()
        .overflow_hidden()
        .font_family(font::mono())
        .text_size(px(font::STATUS_SIZE))
        .when(selected, |row| row.bg(rgba(glass::SELECTION)))
        .when(current && !selected, |row| row.bg(rgba(0xffffff0f)))
        .child(
            div()
                .flex_none()
                .w(px(12.0))
                .text_size(px(10.0))
                .text_color(rgb(if selected {
                    color::FG_ALT
                } else {
                    color::FG_FAINT
                }))
                .child(expander),
        )
        .child(div().flex_none().text_color(rgb(name_color)).child(name))
        .when(!kind.is_empty(), |row| {
            row.child(
                div()
                    .flex_none()
                    .text_color(rgb(color::FG_FAINT))
                    .child(kind),
            )
        })
        .children(value.map(|value| {
            let value_color = value_color(&value);
            div()
                .flex()
                .min_w_0()
                .gap(px(6.0))
                .child(div().text_color(rgb(color::FG_FAINT)).child("="))
                .child(div().truncate().text_color(rgb(value_color)).child(value))
        }))
        .children(tag.map(|(tag, tone)| {
            div()
                .flex_none()
                .px(px(6.0))
                .rounded(px(4.0))
                .text_size(px(font::SMALL_SIZE))
                .text_color(rgb(tone_color(tone, color::FG_DIM)))
                .child(tag)
        }))
        .when(!detail.is_empty(), |row| {
            row.child(
                div()
                    .ml_auto()
                    .pl(px(8.0))
                    .flex_none()
                    .text_size(px(font::SMALL_SIZE))
                    .text_color(rgb(if selected {
                        color::FG_ALT
                    } else {
                        color::FG_FAINT
                    }))
                    .child(detail),
            )
        });
    match action {
        Some(action) => clickable(element.cursor_pointer(), action, cx),
        None => element,
    }
}

/// Strings, numbers and the rest told apart, as the code shows them.
fn value_color(value: &str) -> u32 {
    let first = value.chars().next().unwrap_or_default();
    if first == '"' || first == '\'' || first == '`' {
        color::STRING
    } else if first.is_ascii_digit() || (first == '-' && value.len() > 1) {
        color::CONSTANT
    } else {
        color::FG_BODY
    }
}
