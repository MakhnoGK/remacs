use gpui::{
    AnyElement, Div, HighlightStyle, IntoElement, ParentElement, Pixels, StyledText, div,
    prelude::*, px, rgb, rgba,
};
use remacs_core::{Candidate, DocBlock, Documentation, Popup, SymbolKind};
use remacs_theme::{color, font};

use crate::minibuffer_view::{matches_in, secondary_color};
use crate::widgets::{hsla, kbd, shadow};

const LIST_WIDTH: f32 = 440.0;
const DOCUMENTATION_WIDTH: f32 = 360.0;
const DOCUMENTATION_BODY_MAX_HEIGHT: f32 = 280.0;
const ROW_HEIGHT: f32 = 28.0;
const ROW_GAP: f32 = 2.0;
const LIST_PADDING: f32 = 6.0;
const ROW_PADDING: f32 = 6.0;
const BADGE_SIZE: f32 = 18.0;
const BADGE_GAP: f32 = 8.0;
const DETAIL_MAX_WIDTH: f32 = 200.0;
/// Between the line and the popup, and between the list and the documentation.
pub const POPUP_GAP: f32 = 6.0;
const PANEL_GAP: f32 = 8.0;
/// From the list's edge to its labels, which line up with the completed word.
pub const LABEL_INSET: f32 = 1.0 + LIST_PADDING + ROW_PADDING + BADGE_SIZE + BADGE_GAP;

/// Where a popup goes: `left` is the list's edge, `top`/`bottom` the line's.
pub struct Placement {
    pub left: Pixels,
    pub line_top: Pixels,
    pub line_bottom: Pixels,
    pub window_width: Pixels,
    pub window_height: Pixels,
}

/// The list below the line, or above it when it does not fit; the documentation beside
/// it, on the right if there is room.
pub fn render_popup(popup: &Popup, placement: Placement) -> AnyElement {
    let margin = px(8.0);
    let rows = popup.rows.len() as f32;
    let list_height =
        rows * ROW_HEIGHT + (rows - 1.0).max(0.0) * ROW_GAP + 2.0 * (LIST_PADDING + 1.0);
    let below =
        placement.line_bottom + px(POPUP_GAP + list_height) + margin <= placement.window_height;
    let list_left = placement
        .left
        .min(placement.window_width - px(LIST_WIDTH) - margin)
        .max(margin);
    let room = if below {
        placement.window_height - placement.line_bottom - px(POPUP_GAP) - margin
    } else {
        placement.line_top - px(POPUP_GAP) - margin
    };
    let documentation = popup
        .documentation
        .as_ref()
        .map(|documentation| render_documentation(documentation, popup.hints, room));
    let documentation_right = list_left + px(LIST_WIDTH + PANEL_GAP + DOCUMENTATION_WIDTH)
        <= placement.window_width - margin;
    let (left, children) = match documentation {
        Some(documentation) if !documentation_right => (
            list_left - px(DOCUMENTATION_WIDTH + PANEL_GAP),
            vec![documentation, render_list(popup)],
        ),
        documentation => (
            list_left,
            [render_list(popup)]
                .into_iter()
                .chain(documentation)
                .collect(),
        ),
    };
    let container = div()
        .absolute()
        .left(left)
        .flex()
        .gap(px(PANEL_GAP))
        .font_family(font::UI)
        .text_color(rgb(color::FG_MAIN))
        .children(children);
    if below {
        container
            .top(placement.line_bottom + px(POPUP_GAP))
            .items_start()
    } else {
        container
            .bottom(placement.window_height - placement.line_top + px(POPUP_GAP))
            .items_end()
    }
    .into_any_element()
}

fn panel() -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .bg(rgb(color::BG_POPUP))
        .border_1()
        .border_color(rgba(0xffffff14))
        .rounded(px(12.0))
        .shadow(vec![
            shadow(0x000000b3, 1.0, 2.0, 0.0),
            shadow(0x00000099, 16.0, 40.0, 0.0),
        ])
}

fn render_list(popup: &Popup) -> AnyElement {
    panel()
        .w(px(LIST_WIDTH))
        .p(px(LIST_PADDING))
        .gap(px(ROW_GAP))
        .children(
            popup
                .rows
                .iter()
                .enumerate()
                .map(|(index, row)| render_row(row, popup.selected == Some(index))),
        )
        .into_any_element()
}

fn render_row(row: &Candidate, selected: bool) -> impl IntoElement {
    let secondary = secondary_color(selected);
    let label = StyledText::new(row.label.clone()).with_highlights(matches_in(
        &row.matches,
        0..row.label.len(),
        selected,
    ));
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(BADGE_GAP))
        .h(px(ROW_HEIGHT))
        .px(px(ROW_PADDING))
        .rounded(px(6.0))
        .when(selected, |row| {
            row.bg(rgb(color::BG_SELECTION))
                .shadow(vec![shadow(0x00000073, 1.0, 3.0, 0.0)])
        })
        .font_family(font::MONO)
        .child(match row.kind {
            Some(kind) => render_badge(kind).into_any_element(),
            None => div().flex_none().w(px(BADGE_SIZE)).into_any_element(),
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(font::CODE_SIZE))
                .child(label),
        )
        .when_some(row.detail.clone(), |row, detail| {
            row.child(
                div()
                    .flex_none()
                    .max_w(px(DETAIL_MAX_WIDTH))
                    .truncate()
                    .text_size(px(font::STATUS_SIZE))
                    .text_color(rgb(secondary))
                    .child(detail.into_owned()),
            )
        })
}

/// A letter on a tint of the color code of its kind takes, like `f` for a field.
fn render_badge(kind: SymbolKind) -> impl IntoElement {
    let (letter, tint) = match kind {
        SymbolKind::Function => ("λ", color::FUNCTION),
        SymbolKind::Method => ("m", color::FUNCTION),
        SymbolKind::Field => ("f", color::TYPE),
        SymbolKind::Variable => ("v", color::STRING),
        SymbolKind::Constant => ("c", color::CONSTANT),
        SymbolKind::EnumMember => ("e", color::CONSTANT),
        SymbolKind::Type => ("t", color::TYPE),
        SymbolKind::Interface => ("i", color::TYPE),
        SymbolKind::Module => ("n", color::KEYWORD),
        SymbolKind::Keyword => ("k", color::KEYWORD),
        SymbolKind::Snippet => ("s", color::BUILTIN),
        SymbolKind::Text => ("w", color::FG_DIM),
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(BADGE_SIZE))
        .rounded(px(5.0))
        .bg(rgba(tint << 8 | 0x24))
        .text_size(px(font::SMALL_SIZE))
        .text_color(rgb(tint))
        .child(letter)
}

/// At most `room` tall: the body gives way first, so the hints stay in view.
fn render_documentation(
    documentation: &Documentation,
    hints: &'static [(&'static str, &'static str)],
    room: Pixels,
) -> AnyElement {
    let body = div()
        .flex()
        .flex_col()
        .flex_shrink()
        .min_h_0()
        .gap(px(8.0))
        .max_h(px(DOCUMENTATION_BODY_MAX_HEIGHT))
        .overflow_hidden()
        .children(documentation.blocks.iter().map(render_block));
    panel()
        .w(px(DOCUMENTATION_WIDTH))
        .max_h(room)
        .overflow_hidden()
        .px(px(14.0))
        .py(px(12.0))
        .gap(px(8.0))
        .when(!documentation.title.is_empty(), |panel| {
            panel.child(
                div()
                    .flex_none()
                    .text_size(px(font::SMALL_SIZE))
                    .text_color(rgb(color::FG_DIM))
                    .child(documentation.title.clone()),
            )
        })
        .when_some(documentation.signature.clone(), |panel, signature| {
            panel.child(
                div()
                    .flex_none()
                    .font_family(font::MONO)
                    .text_size(px(font::CODE_SIZE))
                    .line_height(px(20.0))
                    .child(signature),
            )
        })
        .when(!documentation.blocks.is_empty(), |panel| panel.child(body))
        .when(!hints.is_empty(), |panel| panel.child(render_hints(hints)))
        .into_any_element()
}

fn render_block(block: &DocBlock) -> AnyElement {
    match block {
        DocBlock::Text(text) => {
            let (text, code) = code_spans(text);
            div()
                .text_size(px(font::UI_SIZE))
                .line_height(px(19.0))
                .text_color(rgb(color::FG_BODY))
                .child(StyledText::new(text).with_highlights(code))
                .into_any_element()
        }
        DocBlock::Code(code) => div()
            .px(px(8.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .bg(rgb(color::BG_FIELD))
            .overflow_hidden()
            .whitespace_nowrap()
            .font_family(font::MONO)
            .text_size(px(font::STATUS_SIZE))
            .line_height(px(18.0))
            .text_color(rgb(color::FG_ALT))
            .child(code.clone())
            .into_any_element(),
    }
}

/// The text without its backticks, and the ranges they quoted as code.
fn code_spans(text: &str) -> (String, Vec<(std::ops::Range<usize>, HighlightStyle)>) {
    let style = HighlightStyle {
        color: Some(hsla(color::FG_ALT)),
        background_color: Some(rgba(0xffffff12).into()),
        ..Default::default()
    };
    let mut plain = String::with_capacity(text.len());
    let mut code = Vec::new();
    for (index, part) in text.split('`').enumerate() {
        let start = plain.len();
        plain.push_str(part);
        if index % 2 == 1 && !part.is_empty() {
            code.push((start..plain.len(), style));
        }
    }
    (plain, code)
}

fn render_hints(hints: &'static [(&'static str, &'static str)]) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .gap(px(14.0))
        .pt(px(8.0))
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
    use super::*;

    #[test]
    fn code_spans_lose_their_backticks() {
        let (text, code) = code_spans("see `Range` and `a`.");
        assert_eq!(text, "see Range and a.");
        let ranges: Vec<_> = code.into_iter().map(|(range, _)| range).collect();
        assert_eq!(ranges, [4..9, 14..15]);
    }
}
