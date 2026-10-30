use gpui::{Context, Div, ParentElement, StyledText, div, prelude::*, px, rgba, svg};
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
use crate::widgets::{GlassBlur, StyleExt, kbd, shadow};

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
    let mut body = Vec::new();
    for region in view.regions {
        match region {
            ConflictRegion::Text { first, lines, resolved } => {
                let faces = code_faces(&Code::new(lines.join("\n"), mode));
                let rows = numbered(first, &lines, &faces, resolved.is_some()).enumerate();
                body.extend(rows.map(|(index, row)| {
                    row.when_some(resolved.as_ref(), |row, said| {
                        row.bg(rgba(color::vc_added() << 8 | RESOLVED_ALPHA))
                            .when(index == 0, |row| row.child(resolved_chip(said)))
                    })
                }));
            }
            ConflictRegion::Conflict(block) => {
                body.push(render_block(*block, mode, code_faces, cx))
            }
        }
    }
    let well = div().column().flex_1().min_h_0().overflow_hidden().py(px(6.0));
    let well = well.rounded_fill(12.0, glass::well()).bordered(glass::well_border());
    div().column().flex_1().min_w_0().gap(px(8.0)).ui().child(header).child(well.children(body))
}

/// A raised control, as buttons are drawn.
fn control(radius: f32) -> Div {
    div()
        .rounded_fill(radius, glass::control())
        .bordered(glass::control_border())
        .shadow(vec![shadow(glass::shadow(0x66), 1.0, 2.0, 0.0)])
}

fn render_header(view: &ConflictView, cx: &mut Context<EditorView>) -> Div {
    let step = |icon: &'static str, action: Action, cx: &mut Context<EditorView>| {
        let icon = svg().path(icon).size(px(12.0)).fg(color::fg_body());
        let button = control(7.0).flex().items_center().justify_center().size(px(28.0)).child(icon);
        clickable(button, action, cx)
    };
    let name = div().mono().text(font::mono_size(), color::fg_main()).child(view.name.clone());
    let progress = format!("Conflict {} of {}", view.current, view.left);
    let header = div().row(10.0).flex_none().h(px(32.0)).pl(px(12.0)).pr(px(4.0));
    header
        .text_size(px(font::status_size()))
        .child(svg().path(FILE_ICON).size(px(16.0)).fg(color::fg_faint()))
        .child(name)
        .child(div().fg(color::fg_faint()).child(view.directory.clone()))
        .child(div().flex_1())
        .when(view.resolved > 0, |header| {
            let resolved = format!("{} resolved", view.resolved);
            header.child(div().fg(color::fg_added()).child(resolved))
        })
        .child(div().fg(color::fg_body()).child(progress))
        .child(step(CHEVRON_UP_ICON, view.previous.clone(), cx))
        .child(step(CHEVRON_DOWN_ICON, view.next.clone(), cx))
}

/// Code lines numbered from `first`.
fn numbered<'a>(
    first: usize,
    lines: &'a [String],
    faces: &'a Highlights,
    resolved: bool,
) -> impl Iterator<Item = Div> + 'a {
    let number_color = if resolved { color::vc_added() } else { color::fg_line_number() };
    let mut start = 0;
    lines.iter().enumerate().map(move |(index, line)| {
        let range = start..start + line.len();
        start = range.end + 1;
        let number = div().flex_none().w(px(NUMBER_WIDTH)).pr(px(16.0)).text_right();
        let number = number.mono_status(number_color).child((first + index).to_string());
        let styled = StyledText::new(line.clone()).with_highlights(faces_in(faces, range));
        let text = div().flex_1().min_w_0().mono().text(font::mono_size(), color::fg_main());
        let row = div().row(0.0).flex_none().h(px(LINE_HEIGHT)).mx(px(8.0)).rounded(px(6.0));
        row.whitespace_nowrap().overflow_hidden().child(number).child(text.child(styled))
    })
}

fn resolved_chip(said: &str) -> Div {
    let chip = div().flex_none().mr(px(8.0)).px(px(8.0)).py(px(1.0));
    let chip = chip.rounded_fill(5.0, color::vc_added() << 8 | RESOLVED_CHIP_ALPHA);
    chip.text(font::small_size(), color::fg_added()).child(said.to_owned())
}

/// Both sides of a conflict in a card: each under its header with a button keeping it,
/// then buttons keeping both or editing by hand. The current one shows their keys.
fn render_block(
    block: ConflictBlock,
    mode: Option<&'static str>,
    code_faces: &dyn Fn(&Code) -> Highlights,
    cx: &mut Context<EditorView>,
) -> Div {
    let current = block.current;
    let key = |key: &'static str| current.then_some(key);
    let side = |side, tint, bordered, button| {
        render_side(side, (tint, bordered), button, mode, code_faces)
    };
    let keep_ours = key_button("Keep this", key("o"), block.keep_ours, cx);
    let keep_theirs = key_button("Keep this", key("t"), block.keep_theirs, cx);
    let border =
        if current { color::accent() << 8 | CURRENT_BORDER_ALPHA } else { glass::card_border() };
    let footer = div().row(8.0).py(px(8.0)).px(px(12.0)).top_border(glass::divider());
    let footer = footer
        .bg(rgba(glass::card_footer()))
        .child(key_button("Keep both", key("b"), block.keep_both, cx))
        .child(key_button("Edit manually", key("e"), block.edit, cx));
    div()
        .column()
        .flex_none()
        .my(px(6.0))
        .mx(px(8.0))
        .rounded_fill(10.0, glass::surface())
        .overflow_hidden()
        .glass_blur(glass::CARD_BLUR, glass::CARD_SATURATION)
        .bordered(border)
        .shadow(vec![
            shadow(glass::shadow(0x80), 1.0, 2.0, 0.0),
            shadow(glass::shadow(0x66), 10.0, 28.0, 0.0),
        ])
        .children(side(block.ours, color::keyword(), false, keep_ours))
        .children(side(block.theirs, color::accent(), true, keep_theirs))
        .child(footer)
}

/// A side's header, with `button` after it, then its lines.
fn render_side(
    side: ConflictSide,
    (tint, bordered): (u32, bool),
    button: Div,
    mode: Option<&'static str>,
    code_faces: &dyn Fn(&Code) -> Highlights,
) -> Vec<Div> {
    let role_color = match side.chip.kind {
        RefKind::Head => color::fg_ref_head(),
        RefKind::Main => color::fg_ref_main(),
        _ => color::fg_ref_branch(),
    };
    let id = side.id.map(|id| div().mono().fg(color::fg_faint()).child(id));
    let header = div()
        .row(10.0)
        .h(px(34.0))
        .pl(px(12.0))
        .pr(px(8.0))
        .bg(rgba(tint << 8 | SIDE_HEADER_ALPHA))
        .when(bordered, |header| header.top_border(glass::divider()))
        .whitespace_nowrap()
        .status_text(color::fg_kbd())
        .child(ref_chip(side.chip))
        .child(div().fg(role_color).child(side.role))
        .children(id)
        .child(div().min_w_0().truncate().child(side.subject))
        .child(div().flex_1())
        .child(button);
    let faces = code_faces(&Code::new(side.lines.join("\n"), mode));
    let lines = numbered(side.first, &side.lines, &faces, false)
        .map(|row| row.mx_0().rounded_none().bg(rgba(tint << 8 | SIDE_LINE_ALPHA)));
    std::iter::once(header).chain(lines).collect()
}

/// A button with the key that presses it, if it has one now.
fn key_button(
    label: &'static str,
    key: Option<&'static str>,
    action: Action,
    cx: &mut Context<EditorView>,
) -> Div {
    let key_cap = key.map(|key| kbd(key, false).h(px(18.0)).bg(rgba(glass::key_on_control())));
    let button = control(6.0).row(8.0).flex_none().h(px(26.0)).pl(px(10.0));
    let button =
        button.pr(px(if key.is_some() { 5.0 } else { 10.0 })).status_text(color::fg_main());
    clickable(button.child(label).children(key_cap), action, cx)
}
