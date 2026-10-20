use gpui::{
    Context, Div, FontWeight, MouseButton, MouseDownEvent, ParentElement, div, prelude::*, px, rgb,
    rgba, svg,
};
use remacs_core::{Dialog, DialogOption};
use remacs_theme::{color, font, glass};

use crate::assets::TRASH_ICON;
use crate::editor_view::EditorView;
use crate::minibuffer_view::modal;
use crate::widgets::{kbd, list_items, option_chip, shadow, ui_text};

const DIALOG_WIDTH: f32 = 520.0;
/// From the panel's edge to the items, which line up with the title.
const ITEMS_INSET: f32 = 48.0;
/// More would push the buttons off a short window.
const MAX_SHOWN_ITEMS: usize = 8;

/// A y-or-n question with what it is about: its items, an option and the two answers as
/// buttons, which answer it when clicked as `y` and `n` do.
pub fn render_dialog(dialog: Dialog, cx: &mut Context<EditorView>) -> Div {
    let Dialog {
        title,
        explanation,
        items,
        option,
        yes_label,
        no_label,
        destructive,
    } = dialog;
    let header = div()
        .flex()
        .gap(px(14.0))
        .pt(px(12.0))
        .px(px(10.0))
        .pb(px(8.0))
        .when(destructive, |header| header.child(warning_icon()))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .gap(px(4.0))
                .child(div().text_size(px(font::title_size())).child(ui_text(
                    &title,
                    FontWeight::SEMIBOLD,
                    color::fg_main(),
                )))
                .child(
                    div()
                        .text_size(px(font::status_size()))
                        .line_height(px(17.0))
                        .child(ui_text(&explanation, FontWeight::NORMAL, color::fg_dim())),
                ),
        );
    let items = div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .pt(px(4.0))
        .pl(px(ITEMS_INSET))
        .children(list_items(items, MAX_SHOWN_ITEMS, 32.0));
    let buttons = div()
        .flex()
        .justify_end()
        .gap(px(8.0))
        .mt(px(14.0))
        .pt(px(12.0))
        .px(px(6.0))
        .pb(px(4.0))
        .border_t_1()
        .border_color(rgba(glass::ink(0x0f)))
        .child(render_button(no_label.into(), "n", false, cx))
        .child(render_button(yes_label, "y", destructive, cx));
    modal(DIALOG_WIDTH)
        .child(header)
        .child(items)
        .children(option.map(|option| render_option(option, cx)))
        .child(buttons)
}

fn warning_icon() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(34.0))
        .rounded(px(9.0))
        .bg(rgba(color::error() << 8 | 0x24))
        .child(
            svg()
                .path(TRASH_ICON)
                .size(px(16.0))
                .text_color(rgb(color::danger())),
        )
}

fn render_option(option: DialogOption, cx: &mut Context<EditorView>) -> Div {
    let chip = option_chip(option.label, &option.key.to_string(), option.on).on_mouse_down(
        MouseButton::Left,
        cx.listener(|view, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            view.toggle_dialog_option(cx);
        }),
    );
    div()
        .flex()
        .pt(px(12.0))
        .pb(px(2.0))
        .pl(px(ITEMS_INSET + 10.0))
        .child(chip)
}

fn render_button(
    label: String,
    key: &'static str,
    destructive: bool,
    cx: &mut Context<EditorView>,
) -> Div {
    let yes = key == "y";
    let key_cap = kbd(key, false).when(destructive, |key_cap| {
        key_cap
            .bg(rgb(color::bg_danger_kbd()))
            .text_color(rgb(color::fg_on_danger()))
    });
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .h(px(32.0))
        .pl(px(14.0))
        .pr(px(8.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(rgba(glass::ink(0x14)))
        .bg(rgba(glass::control()))
        .shadow(vec![shadow(glass::shadow(0x80), 1.0, 2.0, 0.0)])
        .text_size(px(font::ui_size()))
        .when(destructive, |button| {
            button
                .bg(rgb(color::bg_danger()))
                .border_color(rgba(color::error() << 8 | 0x66))
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                view.answer_question(yes, cx);
            }),
        )
        .child(label)
        .child(key_cap)
}
