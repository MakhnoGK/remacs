use gpui::{
    AnyElement, Div, HighlightStyle, IntoElement, ParentElement, Pixels, Point, ScrollHandle,
    Stateful, StyledText, div, prelude::*, px, rgb, rgba,
};
use remacs_core::{Candidate, Code, DocBlock, Documentation, Popup, SymbolKind};
use remacs_theme::{color, font, glass};

use crate::highlights::Highlights;
use crate::minibuffer_view::{matches_in, secondary_color};
use crate::widgets::{glass_surface, hsla, kbd, shadow};

const LIST_WIDTH: f32 = 440.0;
const DOCUMENTATION_WIDTH: f32 = 360.0;
const DOCUMENTATION_BODY_MAX_HEIGHT: f32 = 280.0;
const BODY_LINE_HEIGHT: f32 = 20.0;
const ROW_HEIGHT: f32 = 28.0;
const ROW_GAP: f32 = 2.0;
const LIST_PADDING: f32 = 6.0;
const ROW_PADDING: f32 = 6.0;
const BADGE_SIZE: f32 = 18.0;
const BADGE_GAP: f32 = 8.0;
const DETAIL_MAX_WIDTH: f32 = 200.0;
/// Between the line and the popup.
const POPUP_GAP: f32 = 6.0;
/// Between the list and the documentation.
const PANEL_GAP: f32 = 8.0;
const WINDOW_MARGIN: f32 = 8.0;
/// From the list's edge to its labels, which line up with the completed word.
const LABEL_INSET: f32 = 1.0 + LIST_PADDING + ROW_PADDING + BADGE_SIZE + BADGE_GAP;
const HOVER_WIDTH: f32 = 520.0;
const HOVER_PADDING: f32 = 16.0;
/// From the documentation's edge to its text, which lines up with the symbol.
const HOVER_INSET: f32 = 1.0 + HOVER_PADDING;
/// Room below the line that is enough for documentation, which is otherwise put where
/// there is more room.
const HOVER_ENOUGH_ROOM: f32 = 320.0;

/// Where a popup goes: `anchor_left` is where the anchor is drawn, `top`/`bottom` the
/// line's.
pub struct Placement {
    pub anchor_left: Pixels,
    pub line_top: Pixels,
    pub line_bottom: Pixels,
    pub window_width: Pixels,
    pub window_height: Pixels,
}

impl Placement {
    /// The popup's left edge, `inset` before the anchor and inside the window.
    fn left(&self, inset: f32, width: f32) -> Pixels {
        (self.anchor_left - px(inset))
            .min(self.window_width - px(width + WINDOW_MARGIN))
            .max(px(WINDOW_MARGIN))
    }

    fn room_below(&self) -> Pixels {
        self.window_height - self.line_bottom - px(POPUP_GAP + WINDOW_MARGIN)
    }

    fn room_above(&self) -> Pixels {
        self.line_top - px(POPUP_GAP + WINDOW_MARGIN)
    }

    /// Positioned below the line, or above it.
    fn place(&self, popup: Div, below: bool) -> Div {
        if below {
            popup.top(self.line_bottom + px(POPUP_GAP)).items_start()
        } else {
            popup
                .bottom(self.window_height - self.line_top + px(POPUP_GAP))
                .items_end()
        }
    }
}

/// What documentation takes from the view: faces for its code, and the scroll position of
/// its body, which the mouse wheel and the plugin's page keys move.
pub struct DocumentationView<'a> {
    pub code_faces: &'a dyn Fn(&Code) -> Highlights,
    pub scroll: &'a ScrollHandle,
}

/// Keeps the shown documentation's scroll position between frames: new documentation
/// starts at the top, and each page the plugin scrolled by moves it a body height.
#[derive(Default)]
pub struct DocumentationScroll {
    pub handle: ScrollHandle,
    /// Says which documentation is shown, and its pages as last applied.
    shown: Option<(DocumentationKey, i32)>,
}

/// The same symbol's documentation, even when more of it is shown.
#[derive(PartialEq)]
struct DocumentationKey {
    anchor: usize,
    container: String,
    kind: String,
    signature: Option<String>,
}

impl DocumentationScroll {
    pub fn update(&mut self, popups: &[Popup]) {
        let shown = popups.iter().find_map(|popup| {
            let documentation = popup.documentation.as_ref()?;
            let key = DocumentationKey {
                anchor: popup.anchor,
                container: documentation.container.clone(),
                kind: documentation.kind.clone(),
                signature: documentation
                    .signature
                    .as_ref()
                    .map(|code| code.text.clone()),
            };
            Some((key, popup.documentation_pages))
        });
        let applied = match (&shown, &self.shown) {
            (Some((key, _)), Some((shown_key, applied))) if key == shown_key => *applied,
            _ => {
                self.handle.set_offset(Point::default());
                0
            }
        };
        if let Some((_, pages)) = &shown {
            let page =
                (self.handle.bounds().size.height - px(BODY_LINE_HEIGHT)).max(px(BODY_LINE_HEIGHT));
            let mut offset = self.handle.offset();
            // gpui clamps the offset to the content when it lays the body out.
            offset.y -= page * (pages - applied) as f32;
            self.handle.set_offset(offset);
        }
        self.shown = shown;
    }
}

/// A list with the selected row's documentation beside it, or documentation alone.
pub fn render_popup(popup: &Popup, placement: Placement, view: &DocumentationView) -> AnyElement {
    match &popup.documentation {
        Some(documentation) if popup.rows.is_empty() => {
            render_hover(documentation, popup.hints, &placement, view)
        }
        _ => render_list_popup(popup, &placement, view),
    }
}

/// Below the symbol, its text in line with the symbol's, or above when there is more room.
fn render_hover(
    documentation: &Documentation,
    hints: &'static [(&'static str, &'static str)],
    placement: &Placement,
    view: &DocumentationView,
) -> AnyElement {
    let (below, above) = (placement.room_below(), placement.room_above());
    let fits_below = below >= px(HOVER_ENOUGH_ROOM) || below >= above;
    let room = if fits_below { below } else { above };
    let has_header = !documentation.container.is_empty() || !documentation.kind.is_empty();
    let panel = panel()
        .w(px(HOVER_WIDTH))
        .max_h(room)
        .overflow_hidden()
        .pt(px(14.0))
        .px(px(HOVER_PADDING))
        .pb(px(12.0))
        .gap(px(10.0))
        .when(has_header, |panel| {
            panel.child(
                div()
                    .flex()
                    .flex_none()
                    .justify_between()
                    .gap(px(12.0))
                    .text_size(px(font::SMALL_SIZE))
                    .text_color(rgb(color::FG_DIM))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(documentation.container.clone()),
                    )
                    .child(div().flex_none().child(documentation.kind.clone())),
            )
        })
        .when_some(documentation.signature.as_ref(), |panel, signature| {
            panel.child(
                div()
                    .flex_none()
                    .px(px(12.0))
                    .py(px(8.0))
                    .rounded(px(8.0))
                    .bg(rgba(glass::FIELD))
                    .border_1()
                    .border_color(rgba(0xffffff0f))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .font_family(font::MONO)
                    .text_size(px(font::CODE_SIZE))
                    .line_height(px(font::CODE_LINE_HEIGHT))
                    .child(render_code(signature, view)),
            )
        })
        .when(!documentation.blocks.is_empty(), |panel| {
            panel.child(render_body(&documentation.blocks, 10.0, view))
        })
        .when(!hints.is_empty(), |panel| {
            panel.child(render_hints(hints).gap(px(18.0)).pt(px(10.0)))
        });
    let container = div()
        .absolute()
        .left(placement.left(HOVER_INSET, HOVER_WIDTH))
        .flex()
        .flex_col()
        .font_family(font::UI)
        .text_color(rgb(color::FG_MAIN))
        .child(panel);
    placement.place(container, fits_below).into_any_element()
}

/// The list below the line, or above it when it does not fit; the documentation beside
/// it, on the right if there is room.
fn render_list_popup(popup: &Popup, placement: &Placement, view: &DocumentationView) -> AnyElement {
    let rows = popup.rows.len() as f32;
    let list_height =
        rows * ROW_HEIGHT + (rows - 1.0).max(0.0) * ROW_GAP + 2.0 * (LIST_PADDING + 1.0);
    let below = placement.room_below() >= px(list_height);
    let list_left = placement.left(LABEL_INSET, LIST_WIDTH);
    let room = if below {
        placement.room_below()
    } else {
        placement.room_above()
    };
    let documentation = popup
        .documentation
        .as_ref()
        .map(|documentation| render_documentation(documentation, popup.hints, room, view));
    let documentation_right = list_left + px(LIST_WIDTH + PANEL_GAP + DOCUMENTATION_WIDTH)
        <= placement.window_width - px(WINDOW_MARGIN);
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
    placement.place(container, below).into_any_element()
}

/// Takes the mouse, so the wheel scrolls the popup and not the buffer behind it.
fn panel() -> Div {
    glass_surface(glass::HINT_RADIUS)
        .occlude()
        .flex()
        .flex_col()
        .flex_none()
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
            row.bg(rgba(glass::SELECTION))
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
    view: &DocumentationView,
) -> AnyElement {
    let title = [documentation.container.as_str(), &documentation.kind]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    panel()
        .w(px(DOCUMENTATION_WIDTH))
        .max_h(room)
        .overflow_hidden()
        .px(px(14.0))
        .py(px(12.0))
        .gap(px(8.0))
        .when(!title.is_empty(), |panel| {
            panel.child(
                div()
                    .flex_none()
                    .text_size(px(font::SMALL_SIZE))
                    .text_color(rgb(color::FG_DIM))
                    .child(title),
            )
        })
        .when_some(documentation.signature.as_ref(), |panel, signature| {
            panel.child(
                div()
                    .flex_none()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .font_family(font::MONO)
                    .text_size(px(font::CODE_SIZE))
                    .line_height(px(BODY_LINE_HEIGHT))
                    .child(render_code(signature, view)),
            )
        })
        .when(!documentation.blocks.is_empty(), |panel| {
            panel.child(
                render_body(&documentation.blocks, 8.0, view)
                    .max_h(px(DOCUMENTATION_BODY_MAX_HEIGHT)),
            )
        })
        .when(!hints.is_empty(), |panel| panel.child(render_hints(hints)))
        .into_any_element()
}

/// Gives way first when the panel is short of room, so the hints stay in view, and
/// scrolls. A heading stays close to the block it introduces.
fn render_body(blocks: &[DocBlock], gap: f32, view: &DocumentationView) -> Stateful<Div> {
    let mut sections = Vec::new();
    let mut blocks = blocks.iter().peekable();
    while let Some(block) = blocks.next() {
        let section = match block {
            DocBlock::Heading(heading) => div()
                .flex()
                .flex_none()
                .flex_col()
                .gap(px(2.0))
                .child(render_heading(heading))
                .children(
                    blocks
                        .next_if(|next| !matches!(next, DocBlock::Heading(_)))
                        .map(|next| render_block(next, view)),
                )
                .into_any_element(),
            block => render_block(block, view),
        };
        sections.push(section);
    }
    div()
        .id("documentation-body")
        .flex()
        .flex_col()
        .flex_shrink()
        .min_h_0()
        .gap(px(gap))
        .overflow_y_scroll()
        .track_scroll(view.scroll)
        .children(sections)
}

/// Colored like the buffer when its mode has a grammar.
fn render_code(code: &Code, view: &DocumentationView) -> StyledText {
    StyledText::new(code.text.clone()).with_highlights((view.code_faces)(code))
}

fn render_heading(heading: &str) -> impl IntoElement {
    let (text, code) = code_spans(heading);
    div()
        .flex_none()
        .text_size(px(font::STATUS_SIZE))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(StyledText::new(text).with_highlights(code))
}

fn render_block(block: &DocBlock, view: &DocumentationView) -> AnyElement {
    match block {
        DocBlock::Heading(heading) => render_heading(heading).into_any_element(),
        DocBlock::Text(text) => {
            let (text, code) = code_spans(text);
            div()
                .flex_none()
                .text_size(px(font::UI_SIZE))
                .line_height(px(BODY_LINE_HEIGHT))
                .text_color(rgb(color::FG_BODY))
                .child(StyledText::new(text).with_highlights(code))
                .into_any_element()
        }
        DocBlock::Code(code) => div()
            .flex_none()
            .px(px(8.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .bg(rgba(glass::FIELD))
            .overflow_hidden()
            .whitespace_nowrap()
            .font_family(font::MONO)
            .text_size(px(font::STATUS_SIZE))
            .line_height(px(18.0))
            .text_color(rgb(color::FG_ALT))
            .child(render_code(code, view))
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

fn render_hints(hints: &'static [(&'static str, &'static str)]) -> Div {
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
