use gpui::{
    AnyElement, Context, Div, IntoElement, ParentElement, StyledText, div, prelude::*, px, rgb,
    rgba, svg,
};
use remacs_core::{
    Action, Code, ConflictBlock, ConflictRegion, ConflictSide, ConflictView, RefKind,
};
use remacs_theme::{color, font, glass};

use crate::assets::{CHEVRON_DOWN_ICON, CHEVRON_UP_ICON, FILE_ICON};
use crate::editor_view::EditorView;
use crate::highlights::Highlights;
use crate::outline_view::ref_chip;
use crate::preview_view::faces_in;
use crate::table_view::clickable;
use crate::widgets::{GlassBlur, kbd, shadow};

const LINE_HEIGHT: f32 = 22.0;
const NUMBER_WIDTH: f32 = 40.0;
/// Alphas over the glass: the resolved lines and their chip, a side's header and lines.
const RESOLVED_ALPHA: u32 = 0x1a;
const RESOLVED_CHIP_ALPHA: u32 = 0x2e;
const SIDE_HEADER_ALPHA: u32 = 0x29;
const SIDE_LINE_ALPHA: u32 = 0x14;
const CURRENT_BORDER_ALPHA: u32 = 0x59;

/// A file's conflicts beside the pane: a header with the file and how far it is, then
/// its lines with each conflict's sides between them.
pub fn render_conflict(
    view: ConflictView,
    code_faces: &dyn Fn(&Code) -> Highlights,
    cx: &mut Context<EditorView>,
) -> Div {
    let mode = view.mode;
    let header = render_header(&view, cx);
    let mut body: Vec<AnyElement> = Vec::new();
    for region in view.regions {
        match region {
            ConflictRegion::Text {
                first,
                lines,
                resolved,
            } => {
                let faces = code_faces(&Code::new(lines.join("\n"), mode));
                body.extend(
                    numbered(
                        first,
                        &lines,
                        &faces,
                        |row, index| {
                            let Some(said) = &resolved else {
                                return row;
                            };
                            row.bg(rgba(color::vc_added() << 8 | RESOLVED_ALPHA))
                                .when(index == 0, |row| row.child(resolved_chip(said)))
                        },
                        resolved.is_some(),
                    )
                    .map(IntoElement::into_any_element),
                );
            }
            ConflictRegion::Conflict(block) => {
                body.push(render_block(*block, mode, code_faces, cx).into_any_element());
            }
        }
    }
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(8.0))
        .font_family(font::UI)
        .child(header)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .py(px(6.0))
                .rounded(px(12.0))
                .bg(rgba(glass::well()))
                .border_1()
                .border_color(rgba(glass::well_border()))
                .children(body),
        )
}

fn render_header(view: &ConflictView, cx: &mut Context<EditorView>) -> Div {
    let step = |icon: &'static str, action: Action, cx: &mut Context<EditorView>| {
        clickable(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.0))
                .rounded(px(7.0))
                .bg(rgba(glass::control()))
                .border_1()
                .border_color(rgba(glass::control_border()))
                .shadow(vec![shadow(glass::shadow(0x66), 1.0, 2.0, 0.0)])
                .child(
                    svg()
                        .path(icon)
                        .size(px(12.0))
                        .text_color(rgb(color::fg_body())),
                ),
            action,
            cx,
        )
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.0))
        .h(px(32.0))
        .pl(px(12.0))
        .pr(px(4.0))
        .text_size(px(font::STATUS_SIZE))
        .child(
            svg()
                .path(FILE_ICON)
                .size(px(16.0))
                .text_color(rgb(color::fg_faint())),
        )
        .child(
            div()
                .font_family(font::mono())
                .text_size(px(font::MONO_SIZE))
                .text_color(rgb(color::fg_main()))
                .child(view.name.clone()),
        )
        .child(
            div()
                .text_color(rgb(color::fg_faint()))
                .child(view.directory.clone()),
        )
        .child(div().flex_1())
        .when(view.resolved > 0, |header| {
            header.child(
                div()
                    .text_color(rgb(color::fg_added()))
                    .child(format!("{} resolved", view.resolved)),
            )
        })
        .child(
            div()
                .text_color(rgb(color::fg_body()))
                .child(format!("Conflict {} of {}", view.current, view.left)),
        )
        .child(step(CHEVRON_UP_ICON, view.previous.clone(), cx))
        .child(step(CHEVRON_DOWN_ICON, view.next.clone(), cx))
}

/// Code lines numbered from `first`, each given to `decorate` with its index.
fn numbered<'a>(
    first: usize,
    lines: &'a [String],
    faces: &'a Highlights,
    decorate: impl Fn(Div, usize) -> Div + 'a,
    resolved: bool,
) -> impl Iterator<Item = Div> + 'a {
    let mut start = 0;
    lines.iter().enumerate().map(move |(index, line)| {
        let range = start..start + line.len();
        start = range.end + 1;
        let row = div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(LINE_HEIGHT))
            .mx(px(8.0))
            .rounded(px(6.0))
            .whitespace_nowrap()
            .overflow_hidden()
            .child(
                div()
                    .flex_none()
                    .w(px(NUMBER_WIDTH))
                    .pr(px(16.0))
                    .text_right()
                    .font_family(font::mono())
                    .text_size(px(font::STATUS_SIZE))
                    .text_color(rgb(if resolved {
                        color::vc_added()
                    } else {
                        color::fg_line_number()
                    }))
                    .child((first + index).to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_family(font::mono())
                    .text_size(px(font::MONO_SIZE))
                    .text_color(rgb(color::fg_main()))
                    .child(StyledText::new(line.clone()).with_highlights(faces_in(faces, range))),
            );
        decorate(row, index)
    })
}

fn resolved_chip(said: &str) -> Div {
    div()
        .flex_none()
        .mr(px(8.0))
        .px(px(8.0))
        .py(px(1.0))
        .rounded(px(5.0))
        .bg(rgba(color::vc_added() << 8 | RESOLVED_CHIP_ALPHA))
        .text_size(px(font::SMALL_SIZE))
        .text_color(rgb(color::fg_added()))
        .child(said.to_owned())
}

/// Both sides of a conflict in a card: each under its header with a button keeping it,
/// then buttons keeping both or editing by hand. The current one shows their keys.
fn render_block(
    block: ConflictBlock,
    mode: Option<&'static str>,
    code_faces: &dyn Fn(&Code) -> Highlights,
    cx: &mut Context<EditorView>,
) -> Div {
    let ConflictBlock {
        current,
        ours,
        theirs,
        keep_ours,
        keep_theirs,
        keep_both,
        edit,
    } = block;
    let key = |key: &'static str| current.then_some(key);
    let mut ours = render_side(ours, (color::keyword(), false), mode, code_faces);
    ours.header = ours
        .header
        .child(key_button("Keep this", key("o"), keep_ours, cx));
    let mut theirs = render_side(theirs, (color::accent(), true), mode, code_faces);
    theirs.header = theirs
        .header
        .child(key_button("Keep this", key("t"), keep_theirs, cx));
    div()
        .flex()
        .flex_col()
        .flex_none()
        .my(px(6.0))
        .mx(px(8.0))
        .rounded(px(10.0))
        .overflow_hidden()
        .bg(rgba(glass::surface()))
        .glass_blur(glass::CARD_BLUR, glass::CARD_SATURATION)
        .border_1()
        .border_color(rgba(if current {
            color::accent() << 8 | CURRENT_BORDER_ALPHA
        } else {
            glass::card_border()
        }))
        .shadow(vec![
            shadow(glass::shadow(0x80), 1.0, 2.0, 0.0),
            shadow(glass::shadow(0x66), 10.0, 28.0, 0.0),
        ])
        .child(ours.header)
        .children(ours.lines)
        .child(theirs.header)
        .children(theirs.lines)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .py(px(8.0))
                .px(px(12.0))
                .border_t_1()
                .border_color(rgba(glass::divider()))
                .bg(rgba(glass::card_footer()))
                .child(key_button("Keep both", key("b"), keep_both, cx))
                .child(key_button("Edit manually", key("e"), edit, cx)),
        )
}

/// A side's header, which takes a button after, and its lines.
struct Side {
    header: Div,
    lines: Vec<Div>,
}

fn render_side(
    side: ConflictSide,
    (tint, bordered): (u32, bool),
    mode: Option<&'static str>,
    code_faces: &dyn Fn(&Code) -> Highlights,
) -> Side {
    let ConflictSide {
        chip,
        role,
        id,
        subject,
        first,
        lines,
    } = side;
    let role_color = match chip.kind {
        RefKind::Head => color::fg_ref_head(),
        RefKind::Main => color::fg_ref_main(),
        _ => color::fg_ref_branch(),
    };
    let header = div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .h(px(34.0))
        .pl(px(12.0))
        .pr(px(8.0))
        .bg(rgba(tint << 8 | SIDE_HEADER_ALPHA))
        .when(bordered, |header| {
            header.border_t_1().border_color(rgba(glass::divider()))
        })
        .whitespace_nowrap()
        .text_size(px(font::STATUS_SIZE))
        .text_color(rgb(color::fg_kbd()))
        .child(ref_chip(chip))
        .child(div().text_color(rgb(role_color)).child(role))
        .children(id.map(|id| {
            div()
                .font_family(font::mono())
                .text_color(rgb(color::fg_faint()))
                .child(id)
        }))
        .child(div().min_w_0().truncate().child(subject))
        .child(div().flex_1());
    let faces = code_faces(&Code::new(lines.join("\n"), mode));
    let lines = numbered(first, &lines, &faces, |row, _| row, false)
        .map(|row| {
            row.mx_0()
                .rounded_none()
                .bg(rgba(tint << 8 | SIDE_LINE_ALPHA))
        })
        .collect();
    Side { header, lines }
}

/// A button with the key that presses it, if it has one now.
fn key_button(
    label: &'static str,
    key: Option<&'static str>,
    action: Action,
    cx: &mut Context<EditorView>,
) -> Div {
    clickable(
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .h(px(26.0))
            .pl(px(10.0))
            .pr(px(if key.is_some() { 5.0 } else { 10.0 }))
            .rounded(px(6.0))
            .bg(rgba(glass::control()))
            .border_1()
            .border_color(rgba(glass::control_border()))
            .shadow(vec![shadow(glass::shadow(0x66), 1.0, 2.0, 0.0)])
            .text_size(px(font::STATUS_SIZE))
            .text_color(rgb(color::fg_main()))
            .child(label)
            .children(key.map(|key| {
                kbd(key, false)
                    .h(px(18.0))
                    .bg(rgba(glass::key_on_control()))
            })),
        action,
        cx,
    )
}
