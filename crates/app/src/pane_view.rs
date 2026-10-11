//! The pane under the text, e.g. a debugger's frames, variables and watches, or a
//! terminal: a toolbar with the switch between panes, then columns of rows or a screen.

use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, Context, Div, FontWeight, ParentElement, Pixels, Size, Styled, div, point,
    prelude::*, px, rgb, rgba, size,
};
use remacs_core::{Editor, Pane, PaneBody, PaneButton, PaneColumn, PaneRow, Tone};
use remacs_theme::{color, font, glass};

use crate::editor_view::EditorView;
use crate::table_view::clickable;
use crate::tabs_view::render_tabs;
use crate::widgets::{kbd, key_caps, segment, segmented, tone_color};

/// Where the pane's bottom is over the window's, clear of the status pill.
pub const PANE_BOTTOM: f32 = 56.0;
/// Between the text and the pane.
pub const PANE_GAP: f32 = 8.0;
const PANE_SIDE: f32 = 16.0;
/// Over the window's glass.
pub const PANE_TINT: u32 = 0xffffff0a;
const TOOLBAR_HEIGHT: f32 = 44.0;
const COLUMN_HEADER_HEIGHT: f32 = 30.0;
const ROW_HEIGHT: f32 = 22.0;
const INDENT: f32 = 14.0;
/// Around a screen in the pane, inside its border.
const SCREEN_SIDE: f32 = 14.0;
const SCREEN_TOP: f32 = 4.0;
const SCREEN_BOTTOM: f32 = 8.0;
/// Of the buttons, state and subtitle on the toolbar's right, for the chips' room.
const TOOLBAR_RIGHT: f32 = 520.0;
const SWITCH_WIDTH: f32 = 170.0;

/// What the pane's frame shows besides the pane itself.
pub struct PaneChrome {
    /// The panes to switch between and their counts, empty when there is one.
    pub switch: Vec<(&'static str, Option<usize>)>,
    pub shown: &'static str,
    /// The pane has the keys.
    pub focused: bool,
    pub window_width: Pixels,
}

/// Where a screen in a pane `height` tall is drawn in a window of `window`.
pub fn screen_bounds(window: Size<Pixels>, height: f32) -> Bounds<Pixels> {
    let top =
        window.height - px(PANE_BOTTOM + height) + px(1.0 + TOOLBAR_HEIGHT + 1.0 + SCREEN_TOP);
    let left = px(PANE_SIDE + 1.0 + SCREEN_SIDE);
    Bounds {
        origin: point(left, top),
        size: size(
            window.width - left * 2.0,
            px(height - 2.0 - TOOLBAR_HEIGHT - 1.0 - SCREEN_TOP - SCREEN_BOTTOM),
        ),
    }
}

/// `screen` is the screen of a pane whose body is one, drawn by the caller.
pub fn render_pane(
    pane: Pane,
    chrome: PaneChrome,
    screen: Option<AnyElement>,
    cx: &mut Context<EditorView>,
) -> Div {
    let rows_fit = ((pane.height - TOOLBAR_HEIGHT - COLUMN_HEADER_HEIGHT) / ROW_HEIGHT)
        .floor()
        .max(1.0) as usize;
    let focused = chrome.focused;
    let toolbar = render_toolbar(&pane, &chrome, cx);
    let body = match pane.body {
        PaneBody::Columns(columns) => div().flex().flex_1().min_h_0().children(
            columns
                .into_iter()
                .map(|column| render_column(column, rows_fit, cx)),
        ),
        PaneBody::Screen(_) => div()
            .flex_1()
            .min_h_0()
            .pt(px(SCREEN_TOP))
            .px(px(SCREEN_SIDE))
            .pb(px(SCREEN_BOTTOM))
            .children(screen),
    };
    div()
        .absolute()
        .left(px(PANE_SIDE))
        .right(px(PANE_SIDE))
        .bottom(px(PANE_BOTTOM))
        .h(px(pane.height))
        .flex()
        .flex_col()
        .rounded(px(12.0))
        .bg(rgba(PANE_TINT))
        .border_1()
        .border_color(rgba(if focused {
            color::ACCENT << 8 | 0x80
        } else {
            0xffffff1f
        }))
        .when(focused, |pane| {
            pane.shadow(vec![gpui::BoxShadow {
                color: rgba(color::ACCENT << 8 | 0x14).into(),
                offset: point(px(0.0), px(0.0)),
                blur_radius: px(0.0),
                spread_radius: px(3.0),
            }])
        })
        .overflow_hidden()
        .font_family(font::UI)
        .text_size(px(font::STATUS_SIZE))
        .child(toolbar)
        .child(body)
}

/// The switch, then a terminal's chips and on the right where it is and how it goes,
/// or a debugger's title and state; the buttons last.
fn render_toolbar(pane: &Pane, chrome: &PaneChrome, cx: &mut Context<EditorView>) -> Div {
    let switch = (!chrome.switch.is_empty()).then(|| render_switch(chrome, cx));
    let state = render_state(pane.state.clone());
    let subtitle = div()
        .font_family(font::mono())
        .text_color(rgb(color::FG_DIM))
        .child(pane.subtitle.clone());
    let toolbar = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .h(px(TOOLBAR_HEIGHT))
        .pl(px(10.0))
        .pr(px(10.0))
        .border_b_1()
        .border_color(rgba(0xffffff12))
        .whitespace_nowrap()
        .children(switch.map(|switch| switch.mr(px(4.0))));
    let toolbar = match pane.tabs.as_ref() {
        Some(tabs) => {
            let switch = if chrome.switch.is_empty() {
                0.0
            } else {
                SWITCH_WIDTH
            };
            let room = f32::from(chrome.window_width) - 2.0 * PANE_SIDE - switch - TOOLBAR_RIGHT;
            toolbar
                .child(render_tabs(tabs.clone(), room, cx))
                .child(div().flex_1())
                .child(subtitle)
                .child(state)
                .child(separator())
        }
        None => toolbar
            .when(chrome.switch.is_empty(), |toolbar| toolbar.pl(px(14.0)))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(font::UI_SIZE))
                    .text_color(rgb(color::FG_MAIN))
                    .child(pane.title.clone()),
            )
            .child(subtitle)
            .child(state)
            .child(div().flex_1()),
    };
    toolbar.children(
        pane.buttons
            .iter()
            .map(|button| render_button(button.clone(), cx)),
    )
}

fn render_switch(chrome: &PaneChrome, cx: &mut Context<EditorView>) -> Div {
    segmented().children(chrome.switch.iter().map(|&(name, count)| {
        let label = match count {
            Some(count) if name != chrome.shown => format!("{name} {count}"),
            _ => name.to_owned(),
        };
        let action: remacs_core::Action = Rc::new(move |editor: &mut Editor| {
            editor.select_pane(name);
            Ok(())
        });
        clickable(
            segment(&label, name == chrome.shown).cursor_pointer(),
            action,
            cx,
        )
    }))
}

fn render_state((state, tone): (String, Tone)) -> Div {
    let state_color = tone_color(tone, color::FG_DIM);
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
        .child(state)
}

fn separator() -> Div {
    div()
        .w(px(1.0))
        .h(px(18.0))
        .mx(px(4.0))
        .bg(rgba(0xffffff1a))
}

pub fn render_button(button: PaneButton, cx: &mut Context<EditorView>) -> Div {
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
