//! Chips for buffers of one kind, like terminals: their state, what happened in them
//! unseen, a `+N` for those that do not fit and a `+` for another.

use std::rc::Rc;

use gpui::{Context, Div, ParentElement, Styled, div, prelude::*, px, rgb, rgba, svg};
use remacs_core::{Editor, Tab, TabState, Tabs};
use remacs_theme::{color, font, glass};

use crate::assets::{CHEVRON_DOWN_ICON, PLUS_ICON};
use crate::editor_view::EditorView;
use crate::table_view::clickable;

const CHIP_HEIGHT: f32 = 26.0;
const GAP: f32 = 6.0;
/// Of a chip's padding, border and dots, beside its label's characters.
const CHIP_CHROME: f32 = 40.0;
const CHAR_WIDTH: f32 = 7.3;
const OVERFLOW_WIDTH: f32 = 52.0;

/// As many chips as fit in `width`, the selected one always; the rest behind `+N`.
pub fn render_tabs(tabs: Tabs, width: f32, cx: &mut Context<EditorView>) -> Div {
    let Tabs { tabs, list, add } = tabs;
    let widths: Vec<f32> = tabs.iter().map(chip_width).collect();
    let selected = tabs.iter().position(|tab| tab.selected);
    let room = width - CHIP_HEIGHT - GAP;
    let mut shown = vec![false; tabs.len()];
    let mut used = selected.map_or(0.0, |index| {
        shown[index] = true;
        widths[index] + GAP
    });
    for (index, chip) in widths.iter().enumerate() {
        if shown[index] {
            continue;
        }
        let hidden_after = shown.iter().filter(|shown| !**shown).count() > 1;
        let reserve = if hidden_after { OVERFLOW_WIDTH } else { 0.0 };
        if used + chip + GAP + reserve > room {
            break;
        }
        used += chip + GAP;
        shown[index] = true;
    }
    let hidden = shown.iter().filter(|shown| !**shown).count();
    let chips: Vec<Div> = tabs
        .into_iter()
        .zip(shown)
        .filter_map(|(tab, shown)| shown.then(|| render_tab(tab, cx)))
        .collect();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(GAP))
        .children(chips)
        .when(hidden > 0, |row| {
            row.child(render_overflow(hidden, list, cx))
        })
        .child(render_add(add, cx))
}

fn chip_width(tab: &Tab) -> f32 {
    CHIP_CHROME + tab.label.chars().count() as f32 * CHAR_WIDTH
}

fn render_tab(tab: Tab, cx: &mut Context<EditorView>) -> Div {
    let Tab {
        label,
        selected,
        state,
        activity,
        bell,
        editing,
        action,
    } = tab;
    let (background, border, text) = match state {
        _ if selected => (glass::SELECTION, 0xffffff24, color::FG_MAIN),
        TabState::Exited(code) if code != 0 => (
            color::ERROR << 8 | 0x1a,
            color::ERROR << 8 | 0x66,
            color::FG_FLAGGED,
        ),
        TabState::Exited(_) => (0xffffff0d, 0xffffff17, color::FG_FAINT),
        _ => (0xffffff0d, 0xffffff17, color::FG_BODY),
    };
    let chip = div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.0))
        .h(px(CHIP_HEIGHT))
        .px(px(9.0))
        .rounded(px(7.0))
        .bg(rgba(background))
        .border_1()
        .border_color(rgba(border))
        .whitespace_nowrap()
        .font_family(font::mono())
        .text_size(px(font::STATUS_SIZE))
        .text_color(rgb(text))
        .cursor_pointer()
        .when(state == TabState::Running, |chip| chip.child(running_dot()))
        .child(editing.unwrap_or(label))
        .map(|chip| match state {
            TabState::Exited(code) => chip.child(exit_badge(code)),
            _ => chip,
        })
        .when(activity || bell, |chip| {
            chip.child(corner_dot(if bell {
                color::WARNING
            } else {
                color::ACCENT
            }))
        });
    clickable(chip, action, cx)
}

fn running_dot() -> Div {
    div()
        .flex_none()
        .size(px(7.0))
        .rounded_full()
        .bg(rgb(color::VC_ADDED))
        .border_2()
        .border_color(rgba(color::VC_ADDED << 8 | 0x2e))
}

fn exit_badge(code: i32) -> Div {
    let (background, text) = if code == 0 {
        (0xffffff14, color::FG_DIM)
    } else {
        (color::ERROR << 8 | 0x38, color::FG_FLAGGED)
    };
    div()
        .flex()
        .items_center()
        .h(px(16.0))
        .px(px(4.0))
        .rounded(px(4.0))
        .bg(rgba(background))
        .text_size(px(10.0))
        .text_color(rgb(text))
        .child(code.to_string())
}

/// New output, or the bell, since the chip's buffer was last shown.
fn corner_dot(tone: u32) -> Div {
    div()
        .absolute()
        .top(px(-3.0))
        .right(px(-3.0))
        .size(px(8.0))
        .rounded_full()
        .bg(rgb(tone))
        .border_2()
        .border_color(rgba(0x101012f2))
}

fn render_overflow(hidden: usize, list: &'static str, cx: &mut Context<EditorView>) -> Div {
    let chip = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(5.0))
        .h(px(CHIP_HEIGHT))
        .px(px(9.0))
        .rounded(px(7.0))
        .bg(rgba(color::ACCENT << 8 | 0x1f))
        .border_1()
        .border_color(rgba(color::ACCENT << 8 | 0x66))
        .font_family(font::mono())
        .text_size(px(font::STATUS_SIZE))
        .text_color(rgb(color::FG_MAIN))
        .cursor_pointer()
        .child(format!("+{hidden}"))
        .child(
            svg()
                .path(CHEVRON_DOWN_ICON)
                .size(px(9.0))
                .text_color(rgb(color::FG_MAIN)),
        );
    clickable(chip, command_action(list), cx)
}

fn render_add(add: &'static str, cx: &mut Context<EditorView>) -> Div {
    let button = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(CHIP_HEIGHT))
        .rounded(px(7.0))
        .border_1()
        .border_dashed()
        .border_color(rgba(0xffffff2e))
        .cursor_pointer()
        .child(
            svg()
                .path(PLUS_ICON)
                .size(px(12.0))
                .text_color(rgb(color::FG_DIM)),
        );
    clickable(button, command_action(add), cx)
}

fn command_action(command: &'static str) -> remacs_core::Action {
    Rc::new(move |editor: &mut Editor| {
        let run = editor.commands().get(command).ok_or("No such command")?.run;
        run(editor)
    })
}
