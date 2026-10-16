use std::ops::Range;

use gpui::{Div, HighlightStyle, ParentElement, StyledText, div, prelude::*, px, rgb, rgba};
use remacs_core::{Code, FileStatus, Preview, PreviewBody, PreviewItem, RowFace};
use remacs_theme::{color, font, glass};

use crate::highlights::Highlights;
use crate::widgets::{hsla, icon, state_color, state_letter};

pub const PREVIEW_WIDTH: f32 = 440.0;
const PADDING: f32 = 8.0;
const HEADER_HEIGHT: f32 = 36.0;
const CODE_LINE_HEIGHT: f32 = 20.0;
const LINE_NUMBER_WIDTH: f32 = 42.0;
const ITEM_HEIGHT: f32 = 24.0;

/// The selected row's contents under its name, and details such as its size below.
pub fn render_preview(preview: Preview, code_faces: &dyn Fn(&Code) -> Highlights) -> Div {
    let Preview {
        icon: preview_icon,
        title,
        kind,
        body,
        details,
        version,
    } = preview;
    let header = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.0))
        .h(px(HEADER_HEIGHT))
        .px(px(PADDING))
        .child(icon(preview_icon, color::fg_faint()))
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_family(font::mono())
                .text_size(px(font::MONO_SIZE))
                .child(title),
        )
        .children(version.map(|(_, status)| state_letter(Some(status.state), false)))
        .child(div().flex_1())
        .child(
            div()
                .flex_none()
                .font_family(font::UI)
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::fg_faint()))
                .child(kind),
        );
    pane()
        .child(header)
        .child(render_body(body, code_faces))
        .child(render_details(details, version))
}

/// The frame of what is shown beside a table's rows.
pub fn pane() -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(PREVIEW_WIDTH))
        .p(px(PADDING))
        .gap(px(PADDING))
        .bg(rgba(glass::pane()))
        .border_1()
        .border_color(rgba(glass::pane_border()))
        .rounded(px(12.0))
        .overflow_hidden()
}

/// Fills the room the header and details leave; what does not fit is cut off.
fn render_body(body: PreviewBody, code_faces: &dyn Fn(&Code) -> Highlights) -> Div {
    let frame = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .py(px(PADDING))
        .rounded(px(8.0))
        .bg(rgba(glass::well()))
        .border_1()
        .border_color(rgba(glass::ink(0x0f)))
        .whitespace_nowrap()
        .font_family(font::mono())
        .text_size(px(font::STATUS_SIZE))
        .line_height(px(CODE_LINE_HEIGHT));
    match body {
        PreviewBody::Code(code) => {
            let faces = code_faces(&code);
            frame.children(numbered_lines(&code.text, &faces))
        }
        PreviewBody::Items(items) => frame.children(items.into_iter().map(render_item)),
        PreviewBody::Message(message) => frame
            .items_center()
            .justify_center()
            .font_family(font::UI)
            .text_color(rgb(color::fg_faint()))
            .child(message),
    }
}

fn numbered_lines<'a>(text: &'a str, faces: &'a Highlights) -> impl Iterator<Item = Div> + 'a {
    let mut start = 0;
    text.split('\n').enumerate().map(move |(index, line)| {
        let range = start..start + line.len();
        start = range.end + 1;
        div()
            .flex()
            .flex_none()
            .child(
                div()
                    .flex_none()
                    .w(px(LINE_NUMBER_WIDTH))
                    .pr(px(14.0))
                    .text_right()
                    .text_color(rgb(color::fg_line_number()))
                    .child((index + 1).to_string()),
            )
            .child(StyledText::new(line.to_owned()).with_highlights(faces_in(faces, range)))
    })
}

/// The faces over `line` of the text, relative to the line.
pub fn faces_in(faces: &Highlights, line: Range<usize>) -> Highlights {
    faces
        .iter()
        .filter(|(range, _)| range.start < line.end && range.end > line.start)
        .map(|(range, style)| {
            let start = range.start.max(line.start) - line.start;
            let end = range.end.min(line.end) - line.start;
            (start..end, *style)
        })
        .collect()
}

fn render_item(item: PreviewItem) -> Div {
    let text_color = match item.face {
        RowFace::Normal => color::fg_main(),
        RowFace::Directory => color::fg_directory(),
        RowFace::Dim => color::fg_faint(),
    };
    let icon_color = match item.face {
        RowFace::Directory => color::fg_directory(),
        _ => color::fg_faint(),
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(PADDING))
        .h(px(ITEM_HEIGHT))
        .px(px(12.0))
        .text_color(rgb(text_color))
        .child(icon(item.icon, icon_color))
        .child(div().min_w_0().truncate().child(item.name))
}

/// Labels and values, then what version control says, e.g. `Modified · +24 −3`.
fn render_details(
    details: Vec<(&'static str, String)>,
    version: Option<(&'static str, FileStatus)>,
) -> Div {
    let plain = details.into_iter().map(|(label, value)| {
        let value = div()
            .min_w_0()
            .truncate()
            .font_family(font::mono())
            .text_color(rgb(color::fg_body()))
            .child(value);
        (label, value)
    });
    let version = version.map(|(name, status)| {
        let mut text = String::new();
        let mut faces = Vec::new();
        let mut push = |part: &str, text_color: Option<u32>| {
            let start = text.len();
            text.push_str(part);
            if let Some(text_color) = text_color {
                let style = HighlightStyle::color(hsla(text_color));
                faces.push((start..text.len(), style));
            }
        };
        push(status.state.label(), Some(state_color(status.state, false)));
        if let Some(lines) = status.lines {
            push(" · ", None);
            push(&format!("+{}", lines.added), Some(color::vc_added()));
            push(" ", None);
            push(&format!("−{}", lines.removed), Some(color::vc_removed()));
        }
        let value = div()
            .flex_none()
            .font_family(font::mono())
            .text_color(rgb(color::fg_body()))
            .child(StyledText::new(text).with_highlights(faces));
        (name, value)
    });
    div()
        .flex()
        .flex_col()
        .flex_none()
        .px(px(2.0))
        .font_family(font::UI)
        .text_size(px(font::STATUS_SIZE))
        .text_color(rgb(color::fg_faint()))
        .children(
            plain
                .chain(version)
                .enumerate()
                .map(|(index, (label, value))| {
                    div()
                        .flex()
                        .justify_between()
                        .gap(px(12.0))
                        .p(px(PADDING))
                        .when(index > 0, |row| {
                            row.border_t_1().border_color(rgba(glass::ink(0x0f)))
                        })
                        .child(div().flex_none().child(label))
                        .child(value)
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::HighlightStyle;

    #[test]
    fn faces_are_cut_to_each_line() {
        let style = HighlightStyle::default();
        let faces = vec![(0..2, style), (3..9, style)];
        assert_eq!(faces_in(&faces, 0..4), [(0..2, style), (3..4, style)]);
        assert_eq!(faces_in(&faces, 5..7), [(0..2, style)]);
        assert!(faces_in(&faces, 10..12).is_empty());
    }
}
