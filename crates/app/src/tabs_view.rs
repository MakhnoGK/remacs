//! Chips for buffers of one kind, like terminals: their state, what happened in them
//! unseen, a `+N` for those that do not fit and a `+` for another.

use std::rc::Rc;

use gpui::{Context, Div, ParentElement, Styled, div, prelude::*, px, rgb, rgba};
use remacs_core::{Editor, Tab, TabState, Tabs};
use remacs_theme::{color, font, glass};

use crate::assets::{CHEVRON_DOWN_ICON, PLUS_ICON};
use crate::editor_view::EditorView;
use crate::table_view::clickable;
use crate::widgets::{StyleExt, svg_icon};

const CHIP_HEIGHT: f32 = 26.0;
const GAP: f32 = 6.0;
/// Of a chip's padding, border and dots, beside its label's characters.
const CHIP_CHROME: f32 = 40.0;
/// A monospace character's advance, in ems.
const CHAR_ADVANCE: f32 = 0.61;
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
    let add_button = div()
        .row(0.0)
        .flex_none()
        .justify_center()
        .size(px(CHIP_HEIGHT))
        .rounded(px(7.0))
        .bordered(glass::ink(0x2e))
        .border_dashed()
        .cursor_pointer()
        .child(svg_icon(PLUS_ICON, 12.0, color::fg_dim()));
    div()
        .row(GAP)
        .flex_none()
        .children(chips)
        .when(hidden > 0, |row| row.child(render_overflow(hidden, list, cx)))
        .child(clickable(add_button, command_action(add), cx))
}

fn chip_width(tab: &Tab) -> f32 {
    CHIP_CHROME + tab.label.chars().count() as f32 * font::status_size() * CHAR_ADVANCE
}

/// A chip `gap` apart from its children, on `background` within `border`.
fn chip(gap: f32, background: u32, border: u32) -> Div {
    div()
        .row(gap)
        .flex_none()
        .bar(CHIP_HEIGHT, 9.0)
        .rounded_fill(7.0, background)
        .bordered(border)
        .cursor_pointer()
}

fn render_tab(tab: Tab, cx: &mut Context<EditorView>) -> Div {
    let Tab { label, selected, state, activity, bell, editing, action } = tab;
    let (background, border, text) = match state {
        _ if selected => (glass::selection(), glass::ink(0x24), color::fg_main()),
        TabState::Exited(code) if code != 0 => {
            (color::error() << 8 | 0x1a, color::error() << 8 | 0x66, color::fg_flagged())
        }
        TabState::Exited(_) => (glass::ink(0x0d), glass::ink(0x17), color::fg_faint()),
        _ => (glass::ink(0x0d), glass::ink(0x17), color::fg_body()),
    };
    let dot = if bell { color::warning() } else { color::accent() };
    let chip = chip(7.0, background, border)
        .relative()
        .whitespace_nowrap()
        .mono_status(text)
        .when(state == TabState::Running, |chip| chip.child(running_dot()))
        .child(editing.unwrap_or(label))
        .map(|chip| match state {
            TabState::Exited(code) => chip.child(exit_badge(code)),
            _ => chip,
        })
        .when(activity || bell, |chip| chip.child(corner_dot(dot)));
    clickable(chip, action, cx)
}

/// A dot `size` pixels across in `fill`, ringed by `ring`, with alpha.
fn dot(size: f32, fill: u32, ring: u32) -> Div {
    div().size(px(size)).rounded_full().bg(rgb(fill)).border_2().border_color(rgba(ring))
}

fn running_dot() -> Div {
    dot(7.0, color::vc_added(), color::vc_added() << 8 | 0x2e).flex_none()
}

fn exit_badge(code: i32) -> Div {
    let (background, text) = if code == 0 {
        (glass::ink(0x14), color::fg_dim())
    } else {
        (color::error() << 8 | 0x38, color::fg_flagged())
    };
    let badge = div().row(0.0).bar(16.0, 4.0).rounded_fill(4.0, background);
    badge.text(10.0, text).child(code.to_string())
}

/// New output, or the bell, since the chip's buffer was last shown.
fn corner_dot(tone: u32) -> Div {
    dot(8.0, tone, color::bg_main() << 8 | 0xf2).absolute().top(px(-3.0)).right(px(-3.0))
}

fn render_overflow(hidden: usize, list: &'static str, cx: &mut Context<EditorView>) -> Div {
    let accent = color::accent() << 8;
    let chip = chip(5.0, accent | 0x1f, accent | 0x66)
        .mono_status(color::fg_main())
        .child(format!("+{hidden}"))
        .child(svg_icon(CHEVRON_DOWN_ICON, 9.0, color::fg_main()));
    clickable(chip, command_action(list), cx)
}

fn command_action(command: &'static str) -> remacs_core::Action {
    Rc::new(move |editor: &mut Editor| {
        let run = editor.commands().get(command).ok_or("No such command")?.run;
        run(editor)
    })
}
