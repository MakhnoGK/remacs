use gpui::{Context, Div, FontWeight, div, prelude::*, px, rgb, rgba};
use remacs_core::{Banner, BannerButton, BannerPart, Icon, Tone, ToolbarButton};
use remacs_theme::{color, font, glass};

use super::{BANNER_HEIGHT, BANNER_TOP, PANE_SIDE, chip_frame, ref_chip};
use crate::editor_view::EditorView;
use crate::table_view::clickable;
use crate::widgets::{StyleExt, icon, shadow, tone_color};

/// What is in progress across the top: its name and refs, the step, the commit applied,
/// how far the conflicts are, and the buttons to go on.
pub(super) fn render_banner(banner: Banner, cx: &mut Context<EditorView>) -> Div {
    let warning = |alpha: u32| color::warning() << 8 | alpha;
    let (state, state_tone) = banner.state;
    let parts = banner.parts.into_iter().map(|part| match part {
        BannerPart::Chip(chip) => ref_chip(chip),
        BannerPart::Word(word) => div().text_color(rgb(color::fg_dim())).child(word),
    });
    let title = div()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(color::fg_main()))
        .child(banner.title);
    let step = banner.step.map(|step| {
        div()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(px(5.0))
            .bg(rgba(warning(0x29)))
            .status_text(color::fg_on_warning())
            .child(step)
    });
    let commit = banner.commit.map(|(id, subject)| {
        div()
            .row(10.0)
            .min_w_0()
            .child(div().flex_none().mono_status(color::fg_faint()).child(id))
            .child(div().min_w_0().truncate().child(subject))
    });
    let state_color = tone_color(state_tone, color::fg_dim());
    let buttons = banner.buttons.into_iter();
    div()
        .absolute()
        .top(px(BANNER_TOP))
        .left(px(PANE_SIDE))
        .right(px(PANE_SIDE))
        .h(px(BANNER_HEIGHT))
        .row(10.0)
        .pl(px(14.0))
        .pr(px(8.0))
        .rounded(px(10.0))
        .bg(rgba(warning(0x1a)))
        .bordered(warning(0x52))
        .font_family(font::ui())
        .text(font::ui_size(), color::fg_body())
        .whitespace_nowrap()
        .child(icon(Icon::Branch, color::warning()).size(px(16.0)))
        .child(title)
        .children(parts)
        .children(step)
        .children(commit)
        .child(div().flex_1())
        .child(div().flex_none().status_text(state_color).child(state))
        .children(buttons.map(|button| render_banner_button(button, cx)))
}

/// Danger for one that throws work away, Accent for the one to press next; a disabled
/// one cannot be pressed.
fn render_banner_button(button: BannerButton, cx: &mut Context<EditorView>) -> Div {
    let enabled = button.enabled;
    let (danger_border, accent_border) = (color::error() << 8 | 0x4d, color::accent() << 8 | 0x73);
    let (background, border, text) = match button.tone {
        _ if !enabled => (glass::disabled(), glass::ink(0x14), color::fg_muted()),
        Tone::Danger => (glass::control(), danger_border, color::fg_flagged()),
        Tone::Accent => (glass::selection(), accent_border, color::fg_main()),
        _ => (glass::control(), glass::control_border(), color::fg_main()),
    };
    let lift = vec![shadow(glass::shadow(0x80), 1.0, 2.0, 0.0)];
    chip_frame(28.0, 12.0, 7.0)
        .bg(rgba(background))
        .bordered(border)
        .status_text(text)
        .when(enabled, |element| element.shadow(lift))
        .child(button.label)
        .when(enabled, |element| clickable(element, button.action, cx))
}

/// Buttons at the right of the title bar, with a badge for what each would move.
pub fn render_toolbar(buttons: Vec<ToolbarButton>, cx: &mut Context<EditorView>) -> Div {
    div()
        .absolute()
        .top(px(9.0))
        .right(px(14.0))
        .flex()
        .gap(px(8.0))
        .children(buttons.into_iter().map(|button| toolbar_button(button, cx)))
}

fn toolbar_button(button: ToolbarButton, cx: &mut Context<EditorView>) -> Div {
    let enabled = button.enabled;
    let text = enabled.then(color::fg_body).unwrap_or_else(color::fg_muted);
    let count = button.count.filter(|_| enabled).map(|count| {
        div()
            .px(px(5.0))
            .rounded(px(4.0))
            .bg(rgba(glass::selection()))
            .font_family(font::mono())
            .text(font::small_size(), color::fg_main())
            .line_height(px(16.0))
            .child(count.to_string())
    });
    div()
        .row(6.0)
        .h(px(26.0))
        .px(px(10.0))
        .rounded(px(7.0))
        .bg(rgba(glass::control()))
        .bordered(glass::control_border())
        .shadow(vec![shadow(glass::shadow(0x99), 1.0, 2.0, 0.0)])
        .font_family(font::ui())
        .status_text(text)
        .child(icon(button.icon, text))
        .child(button.label)
        .children(count)
        .when(enabled, |element| clickable(element, button.action, cx))
}
