use std::sync::Arc;

use gpui::{
    BoxShadow, Div, FontFeatures, HighlightStyle, Hsla, IntoElement, ParentElement, Rgba, Styled,
    Svg, canvas, div, linear_color_stop, linear_gradient, point, prelude::FluentBuilder, px, rgb,
    rgba, svg,
};
use remacs_core::{FileState, Icon, LineCounts, ListItem, Mark, Tone};
use remacs_theme::{color, font, glass};

use crate::assets::{
    BRANCH_ICON, CHECK_ICON, CHEVRON_DOWN_ICON, ERROR_ICON, FILE_ICON, FOLDER_ICON, INFO_ICON,
    PACKAGE_ICON, PLUS_ICON, PULL_ICON, PUSH_ICON, REMOTE_ICON, RERUN_ICON, RUN_ICON, SCRIPT_ICON,
    STOP_ICON, TERMINAL_ICON, WARNING_ICON,
};

pub fn hsla(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// What the code font may do with its characters; fonts join them through either feature.
pub fn code_font_features() -> FontFeatures {
    if font::code().ligatures {
        FontFeatures::default()
    } else {
        FontFeatures(Arc::new(vec![("calt".into(), 0), ("liga".into(), 0)]))
    }
}

pub const SCROLL_FADE_SIZE: f32 = 32.0;

/// Whether there is more of a list than is shown, before and after it.
#[derive(Clone, Copy, Default)]
pub struct Scrolled {
    pub above: bool,
    pub below: bool,
}

#[derive(Clone, Copy)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// `list`, its content dissolving into `under`, what it is drawn on, at the edges with more
/// beyond them.
pub fn fade_scrolled(list: Div, scrolled: Scrolled, under: Hsla) -> Div {
    list.relative()
        .when(scrolled.above, |list| {
            list.child(edge_fade(Edge::Top, SCROLL_FADE_SIZE, under))
        })
        .when(scrolled.below, |list| {
            list.child(edge_fade(Edge::Bottom, SCROLL_FADE_SIZE, under))
        })
}

/// `list` moved up by `shift` pixels, its first row partly scrolled past, in a box that
/// takes the room left and clips it.
pub fn scrolled_by(list: Div, shift: f32) -> Div {
    div()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .child(list.relative().top(px(-shift)))
}

/// Mixes what its parent drew within `size` of `edge` toward `under`: wholly at the edge,
/// less and less away from it. Unlike a gradient over the content, it lets the glass show.
pub fn edge_fade(edge: Edge, size: f32, under: Hsla) -> impl IntoElement {
    let angle = match edge {
        Edge::Top => 180.0,
        Edge::Bottom => 0.0,
        Edge::Left => 90.0,
        Edge::Right => 270.0,
    };
    let amount = linear_gradient(
        angle,
        linear_color_stop(hsla(0x000000), 0.0),
        linear_color_stop(rgba(0x00000000), 1.0),
    );
    let fade = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| window.paint_fade(bounds, amount, under),
    )
    .absolute();
    match edge {
        Edge::Top => fade.left_0().right_0().h(px(size)).top_0(),
        Edge::Bottom => fade.left_0().right_0().h(px(size)).bottom_0(),
        Edge::Left => fade.top_0().bottom_0().w(px(size)).left_0(),
        Edge::Right => fade.top_0().bottom_0().w(px(size)).right_0(),
    }
}

/// `top` drawn over `bottom`, as one color.
pub fn over(top: u32, bottom: u32) -> Hsla {
    let (top, bottom) = (rgba(top), rgba(bottom));
    let alpha = top.a + bottom.a * (1.0 - top.a);
    if alpha == 0.0 {
        return top.into();
    }
    let mix = |top_channel: f32, bottom_channel: f32| {
        (top_channel * top.a + bottom_channel * bottom.a * (1.0 - top.a)) / alpha
    };
    Rgba {
        r: mix(top.r, bottom.r),
        g: mix(top.g, bottom.g),
        b: mix(top.b, bottom.b),
        a: alpha,
    }
    .into()
}

/// File icons take the text color they stand next to; severity icons keep their own.
pub fn icon(icon: Icon, text_color: u32) -> Svg {
    let (path, size, color) = match icon {
        Icon::File => (FILE_ICON, 16.0, text_color),
        Icon::Directory => (FOLDER_ICON, 16.0, text_color),
        Icon::Error => (ERROR_ICON, 10.0, color::ERROR),
        Icon::Warning => (WARNING_ICON, 10.0, color::WARNING),
        Icon::Info => (INFO_ICON, 10.0, color::INFO),
        Icon::Branch => (BRANCH_ICON, 12.0, text_color),
        Icon::Remote => (REMOTE_ICON, 14.0, text_color),
        Icon::Create => (PLUS_ICON, 14.0, text_color),
        Icon::Pull => (PULL_ICON, 12.0, text_color),
        Icon::Push => (PUSH_ICON, 12.0, text_color),
        Icon::Run => (RUN_ICON, 12.0, color::VC_ADDED),
        Icon::Stop => (STOP_ICON, 12.0, text_color),
        Icon::Rerun => (RERUN_ICON, 12.0, text_color),
        Icon::Package => (PACKAGE_ICON, 12.0, text_color),
        Icon::Script => (SCRIPT_ICON, 12.0, text_color),
        Icon::Terminal => (TERMINAL_ICON, 12.0, text_color),
    };
    svg()
        .path(path)
        .flex_none()
        .size(px(size))
        .text_color(rgb(color))
}

/// The theme's color for `tone`; a plain one takes `plain`.
pub fn tone_color(tone: Tone, plain: u32) -> u32 {
    match tone {
        Tone::Plain => plain,
        Tone::Accent => color::ACCENT,
        Tone::Danger => color::DANGER,
        Tone::Warning => color::WARNING,
        Tone::Faint => color::FG_FAINT,
        Tone::Special => color::FUNCTION,
        Tone::Success => color::FG_ADDED,
    }
}

/// A check for a marked row, a `D` for one flagged for deletion.
pub fn mark_badge(mark: Mark) -> Div {
    let badge = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(4.0))
        .shadow(vec![shadow(0x00000080, 1.0, 2.0, 0.0)]);
    match mark {
        Mark::Marked => badge.size(px(14.0)).bg(rgb(color::ACCENT)).child(
            svg()
                .path(CHECK_ICON)
                .size(px(10.0))
                .text_color(rgb(color::BG_MAIN)),
        ),
        Mark::Flagged => badge
            .size(px(16.0))
            .bg(rgb(color::BG_DANGER))
            .font_family(font::mono())
            .text_size(px(10.0))
            .line_height(px(16.0))
            .text_color(rgb(color::FG_ON_DANGER))
            .child("D"),
    }
}

/// The color of a file's version control state; on a selected row a modified one takes
/// the selection's text color.
pub fn state_color(state: FileState, selected: bool) -> u32 {
    match state {
        FileState::Added | FileState::Resolved => color::VC_ADDED,
        FileState::Modified | FileState::Renamed if selected => color::FG_ALT,
        FileState::Modified | FileState::Renamed => color::VC_MODIFIED,
        FileState::Deleted => color::VC_REMOVED,
        FileState::Conflicted => color::ERROR,
        FileState::Untracked => color::FG_DIM,
    }
}

/// A file's version control state as its letter, centered in its slot.
pub fn state_letter(state: Option<FileState>, selected: bool) -> Div {
    div()
        .flex_none()
        .text_center()
        .font_family(font::mono())
        .text_size(px(font::STATUS_SIZE))
        .when_some(state, |slot, state| {
            slot.text_color(rgb(state_color(state, selected)))
                .child(state.letter())
        })
}

/// `+24 −3`, the zero side left out.
pub fn line_counts(lines: LineCounts, selected: bool) -> Div {
    let (added, removed) = if selected {
        (color::VC_ADDED_ON_SELECTION, color::VC_REMOVED_ON_SELECTION)
    } else {
        (color::VC_ADDED, color::VC_REMOVED)
    };
    let count = |text: String, text_color| div().text_color(rgb(text_color)).child(text);
    div()
        .flex()
        .flex_none()
        .gap(px(10.0))
        .font_family(font::mono())
        .text_size(px(font::STATUS_SIZE))
        .when(lines.added > 0, |counts| {
            counts.child(count(format!("+{}", lines.added), added))
        })
        .when(lines.removed > 0, |counts| {
            counts.child(count(format!("−{}", lines.removed), removed))
        })
}

/// Rows of `items` `row_height` tall, at most `max` of them and a line saying how many
/// more there are.
pub fn list_items(items: Vec<ListItem>, max: usize, row_height: f32) -> Vec<Div> {
    let hidden = items.len().saturating_sub(max);
    let rows = items.into_iter().take(max).map(|item| {
        let row = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .h(px(row_height))
            .px(px(10.0))
            .children(item.mark.map(mark_badge))
            .children(item.icon.map(|item_icon| icon(item_icon, color::FG_DIM)));
        if let Some(state) = item.state {
            // A changed file: its name, then where it is and how many lines changed.
            return row
                .child(state_letter(Some(state), false).w(px(12.0)))
                .child(
                    div()
                        .flex_none()
                        .font_family(font::mono())
                        .text_size(px(font::MONO_SIZE))
                        .child(item.label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(font::STATUS_SIZE))
                        .text_color(rgb(color::FG_DIM))
                        .child(item.detail),
                )
                .children(item.lines.map(|lines| line_counts(lines, false)));
        }
        row.child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(font::mono())
                .text_size(px(font::MONO_SIZE))
                .text_color(rgb(color::FG_KBD))
                .child(item.label),
        )
        .child(
            div()
                .flex_none()
                .font_family(font::mono())
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::FG_DIM))
                .child(item.detail),
        )
    });
    let more = (hidden > 0).then(|| {
        div()
            .px(px(10.0))
            .py(px(4.0))
            .text_size(px(font::STATUS_SIZE))
            .text_color(rgb(color::FG_DIM))
            .child(format!("and {hidden} more"))
    });
    rows.chain(more).collect()
}

/// A sunken strip of `segment`s, one of them selected.
pub fn segmented() -> Div {
    div()
        .flex()
        .flex_none()
        .p(px(2.0))
        .rounded(px(8.0))
        .bg(rgba(glass::FIELD))
        .border_1()
        .border_color(rgba(0xffffff0f))
}

pub fn segment(label: &str, selected: bool) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(24.0))
        .px(px(10.0))
        .rounded(px(6.0))
        .font_family(font::UI)
        .text_size(px(font::STATUS_SIZE))
        .text_color(rgb(color::FG_DIM))
        .when(selected, |segment| {
            segment
                .bg(rgba(glass::SEGMENT))
                .text_color(rgb(color::FG_MAIN))
                .shadow(vec![shadow(0x00000080, 1.0, 2.0, 0.0)])
        })
        .child(label.to_owned())
}

/// A setting's chip, lit while it is on, with the key that switches it.
pub fn option_chip(label: &str, key: &str, on: bool) -> Div {
    toggle_chip(label, on).child(kbd(key, false))
}

/// A choice's chip: its label and value, which a click changes.
pub fn choice_chip(label: &str, value: &str) -> Div {
    toggle_chip(&format!("{label}:"), false)
        .child(
            div()
                .font_family(font::mono())
                .text_color(rgb(color::FG_MAIN))
                .child(value.to_owned()),
        )
        .child(
            svg()
                .path(CHEVRON_DOWN_ICON)
                .size(px(10.0))
                .text_color(rgb(color::FG_KBD)),
        )
}

/// A setting's chip, lit and checked while it is on.
pub fn toggle_chip(label: &str, on: bool) -> Div {
    let (background, border, text) = if on {
        (
            rgba(color::ACCENT << 8 | 0x29),
            rgba(color::ACCENT << 8 | 0x66),
            color::FG_ALT,
        )
    } else {
        (
            rgba(glass::CONTROL),
            rgba(glass::CONTROL_BORDER),
            color::FG_KBD,
        )
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .h(px(26.0))
        .px(px(10.0))
        .rounded(px(6.0))
        .bg(background)
        .border_1()
        .border_color(border)
        .font_family(font::UI)
        .text_size(px(font::STATUS_SIZE))
        .text_color(rgb(text))
        .when(on, |chip| {
            chip.child(svg().path(CHECK_ICON).size(px(10.0)).text_color(rgb(text)))
        })
        .child(label.to_owned())
}

/// A cap for each key of a sequence, e.g. two for `C-c C-c`.
pub fn key_caps(sequence: &str) -> impl Iterator<Item = Div> + '_ {
    sequence.split_whitespace().map(|key| kbd(key, false))
}

/// The block cursor: the character under it drawn inverted.
pub fn cursor_style() -> HighlightStyle {
    HighlightStyle {
        background_color: Some(hsla(color::FG_MAIN)),
        ..cursor_text_style()
    }
}

/// The character under a block cursor drawn on its own, under the text.
pub fn cursor_text_style() -> HighlightStyle {
    HighlightStyle {
        color: Some(hsla(color::BG_MAIN)),
        ..Default::default()
    }
}

/// A floating surface of glass, showing the editor under it blurred.
pub fn glass_surface(radius: f32) -> Div {
    div()
        .bg(rgba(glass::SURFACE))
        .backdrop_filter(px(glass::SURFACE_BLUR), glass::SURFACE_SATURATION)
        .border_1()
        .border_color(rgba(glass::SURFACE_BORDER))
        .rounded(px(radius))
        .shadow(vec![
            shadow(0x00000080, 1.0, 2.0, 0.0),
            shadow(0x00000080, 24.0, 60.0, 0.0),
        ])
}

/// A modal surface: thicker glass than a floating one, lifted further.
pub fn modal_surface() -> Div {
    div()
        .bg(rgba(glass::MODAL))
        .backdrop_filter(px(glass::MODAL_BLUR), glass::SURFACE_SATURATION)
        .border_1()
        .border_color(rgba(glass::MODAL_BORDER))
        .rounded(px(glass::SURFACE_RADIUS))
        .shadow(vec![
            shadow(0x00000080, 1.0, 2.0, 0.0),
            shadow(0x00000099, 30.0, 80.0, 0.0),
        ])
}

pub fn shadow(color: u32, y: f32, blur: f32, spread: f32) -> BoxShadow {
    BoxShadow {
        color: rgba(color).into(),
        offset: point(px(0.0), px(y)),
        blur_radius: px(blur),
        spread_radius: px(spread),
    }
}

pub fn modified_dot() -> impl IntoElement {
    div()
        .flex_none()
        .size(px(7.0))
        .rounded_full()
        .bg(rgb(color::WARNING))
        .shadow(vec![shadow(0xfec43f2e, 0.0, 0.0, 2.0)])
}

/// A translucent floating bar over the bottom of the window, like the status pill.
pub fn pill() -> Div {
    div()
        .absolute()
        .bottom(px(14.0))
        .flex()
        .items_center()
        .gap(px(2.0))
        .p(px(3.0))
        .rounded(px(9.0))
        .bg(rgba(glass::PILL))
        .backdrop_filter(px(glass::PILL_BLUR), 1.0)
        .border_1()
        .border_color(rgba(glass::PILL_BORDER))
        .shadow(vec![shadow(0x00000059, 4.0, 14.0, 0.0)])
        .text_size(px(font::STATUS_SIZE))
        .text_color(rgb(color::FG_FAINT))
}

pub fn pill_separator() -> Div {
    div().w(px(1.0)).h(px(14.0)).bg(rgba(0xffffff14))
}

/// A key cap, e.g. `C-x`.
pub fn kbd(key: &str, selected: bool) -> Div {
    let (background, foreground) = if selected {
        (rgb(color::BG_KBD_SELECTED), color::FG_MAIN)
    } else {
        (rgba(glass::CONTROL), color::FG_KBD)
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(20.0))
        .px(px(6.0))
        .rounded(px(5.0))
        .bg(background)
        .border_1()
        .border_color(rgba(glass::CONTROL_BORDER))
        .shadow(vec![shadow(0x00000099, 1.0, 0.0, 0.0)])
        .font_family(font::mono())
        .text_size(px(font::SMALL_SIZE))
        .text_color(rgb(foreground))
        .child(key.to_owned())
}
