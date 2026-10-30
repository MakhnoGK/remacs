use gpui::{Context, Div, ParentElement, div, prelude::*, px, rgb, rgba, svg};
use remacs_core::{Dialog, DialogOption};
use remacs_theme::{color, font, glass};

use crate::assets::TRASH_ICON;
use crate::editor_view::EditorView;
use crate::minibuffer_view::{PanelStyle, modal, plain_text, title_text};
use crate::widgets::{PressExt, StyleExt, kbd, list_items, option_chip, shadow};

const DIALOG_WIDTH: f32 = 520.0;
/// From the panel's edge to the items, which line up with the title.
const ITEMS_INSET: f32 = 48.0;
/// More would push the buttons off a short window.
const MAX_SHOWN_ITEMS: usize = 8;

/// A y-or-n question with what it is about: its items, an option and the two answers as
/// buttons, which answer it when clicked as `y` and `n` do.
pub fn render_dialog(dialog: Dialog, cx: &mut Context<EditorView>) -> Div {
    let destructive = dialog.destructive;
    let explanation = plain_text(&dialog.explanation, font::status_size(), color::fg_dim());
    let header = div()
        .flex()
        .gap(px(14.0))
        .pad(12.0, 10.0, 8.0)
        .when(destructive, |header| header.child(warning_icon()))
        .child(
            div()
                .column()
                .min_w_0()
                .gap(px(4.0))
                .child(title_text(&dialog.title))
                .child(explanation.line_height(px(17.0))),
        );
    let items = div()
        .column()
        .gap(px(2.0))
        .pt(px(4.0))
        .pl(px(ITEMS_INSET))
        .children(list_items(dialog.items, MAX_SHOWN_ITEMS, 32.0));
    let buttons = div()
        .flex()
        .justify_end()
        .gap(px(8.0))
        .mt(px(14.0))
        .pad(12.0, 6.0, 4.0)
        .top_border(glass::ink(0x0f))
        .child(render_button(dialog.no_label.into(), false, false, cx))
        .child(render_button(dialog.yes_label, true, destructive, cx));
    modal(DIALOG_WIDTH)
        .child(header)
        .child(items)
        .children(dialog.option.map(|option| render_option(option, cx)))
        .child(buttons)
}

fn warning_icon() -> Div {
    let trash = svg().path(TRASH_ICON).size(px(16.0));
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(34.0))
        .rounded_fill(9.0, color::error() << 8 | 0x24)
        .child(trash.text_color(rgb(color::danger())))
}

fn render_option(option: DialogOption, cx: &mut Context<EditorView>) -> Div {
    let chip = option_chip(option.label, &option.key.to_string(), option.on)
        .on_press(cx, |view, cx| view.toggle_dialog_option(cx));
    div()
        .flex()
        .pt(px(12.0))
        .pb(px(2.0))
        .pl(px(ITEMS_INSET + 10.0))
        .child(chip)
}

/// The answer `yes` or not, with its key.
fn render_button(label: String, yes: bool, destructive: bool, cx: &mut Context<EditorView>) -> Div {
    let key_cap = kbd(if yes { "y" } else { "n" }, false).when(destructive, |key_cap| {
        key_cap
            .bg(rgb(color::bg_danger_kbd()))
            .fg(color::fg_on_danger())
    });
    div()
        .row(10.0)
        .h(px(32.0))
        .pl(px(14.0))
        .pr(px(8.0))
        .bordered(glass::ink(0x14))
        .rounded_fill(8.0, glass::control())
        .shadow(vec![shadow(glass::shadow(0x80), 1.0, 2.0, 0.0)])
        .text_size(px(font::ui_size()))
        .when(destructive, |button| {
            button
                .bg(rgb(color::bg_danger()))
                .border_color(rgba(color::error() << 8 | 0x66))
        })
        .on_press(cx, move |view, cx| view.answer_question(yes, cx))
        .child(label)
        .child(key_cap)
}
