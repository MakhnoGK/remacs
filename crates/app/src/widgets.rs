use std::ops::Range;
use std::sync::Arc;

use gpui::{
    BoxShadow, Div, FontFeatures, HighlightStyle, Hsla, IntoElement, ParentElement, Rgba, Styled,
    StyledText, Svg, canvas, div, linear_color_stop, linear_gradient, point,
    prelude::FluentBuilder, px, rgb, rgba, svg, transparent_black,
};
use remacs_core::{FileState, Icon, LineCounts, ListItem, Mark, Tone};
use remacs_theme::{color, cursor, font, glass};

use crate::assets::{
    BRANCH_ICON, CHECK_ICON, CHEVRON_DOWN_ICON, ERROR_ICON, FILE_ICON, FOLDER_ICON, INFO_ICON,
    PACKAGE_ICON, PLUS_ICON, PULL_ICON, PUSH_ICON, REMOTE_ICON, RERUN_ICON, RUN_ICON, SCRIPT_ICON,
    STOP_ICON, TERMINAL_ICON, WARNING_ICON,
};
use crate::highlights::{Highlights, slice};

pub fn hsla(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub trait GlassBlur: Styled {
    /// Blurs what is under the element by `radius` pixels, unless `ui.toml` turned
    /// blurring off.
    fn glass_blur(self, radius: f32, saturation: f32) -> Self {
        if glass::blurs() {
            self.backdrop_filter(px(radius), saturation)
        } else {
            self
        }
    }
}

impl<E: Styled> GlassBlur for E {}

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
        Icon::Error => (ERROR_ICON, 10.0, color::error()),
        Icon::Warning => (WARNING_ICON, 10.0, color::warning()),
        Icon::Info => (INFO_ICON, 10.0, color::info()),
        Icon::Branch => (BRANCH_ICON, 12.0, text_color),
        Icon::Remote => (REMOTE_ICON, 14.0, text_color),
        Icon::Create => (PLUS_ICON, 14.0, text_color),
        Icon::Pull => (PULL_ICON, 12.0, text_color),
        Icon::Push => (PUSH_ICON, 12.0, text_color),
        Icon::Run => (RUN_ICON, 12.0, color::vc_added()),
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
        Tone::Accent => color::accent(),
        Tone::Danger => color::danger(),
        Tone::Warning => color::warning(),
        Tone::Faint => color::fg_faint(),
        Tone::Special => color::function(),
        Tone::Success => color::fg_added(),
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
        .shadow(vec![shadow(glass::shadow(0x80), 1.0, 2.0, 0.0)]);
    match mark {
        Mark::Marked => badge.size(px(14.0)).bg(rgb(color::accent())).child(
            svg()
                .path(CHECK_ICON)
                .size(px(10.0))
                .text_color(rgb(color::bg_main())),
        ),
        Mark::Flagged => badge
            .size(px(16.0))
            .bg(rgb(color::bg_danger()))
            .font_family(font::mono())
            .text_size(px(10.0))
            .line_height(px(16.0))
            .text_color(rgb(color::fg_on_danger()))
            .child("D"),
    }
}

/// The color of a file's version control state; on a selected row a modified one takes
/// the selection's text color.
pub fn state_color(state: FileState, selected: bool) -> u32 {
    match state {
        FileState::Added | FileState::Resolved => color::vc_added(),
        FileState::Modified | FileState::Renamed if selected => color::fg_alt(),
        FileState::Modified | FileState::Renamed => color::vc_modified(),
        FileState::Deleted => color::vc_removed(),
        FileState::Conflicted => color::error(),
        FileState::Untracked => color::fg_dim(),
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
        (
            color::vc_added_on_selection(),
            color::vc_removed_on_selection(),
        )
    } else {
        (color::vc_added(), color::vc_removed())
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
            .children(item.icon.map(|item_icon| icon(item_icon, color::fg_dim())));
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
                        .text_color(rgb(color::fg_dim()))
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
                .text_color(rgb(color::fg_kbd()))
                .child(item.label),
        )
        .child(
            div()
                .flex_none()
                .font_family(font::mono())
                .text_size(px(font::STATUS_SIZE))
                .text_color(rgb(color::fg_dim()))
                .child(item.detail),
        )
    });
    let more = (hidden > 0).then(|| {
        div()
            .px(px(10.0))
            .py(px(4.0))
            .text_size(px(font::STATUS_SIZE))
            .text_color(rgb(color::fg_dim()))
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
        .bg(rgba(glass::field()))
        .border_1()
        .border_color(rgba(glass::ink(0x0f)))
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
        .text_color(rgb(color::fg_dim()))
        .when(selected, |segment| {
            segment
                .bg(rgba(glass::segment()))
                .text_color(rgb(color::fg_main()))
                .shadow(vec![shadow(glass::shadow(0x80), 1.0, 2.0, 0.0)])
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
                .text_color(rgb(color::fg_main()))
                .child(value.to_owned()),
        )
        .child(
            svg()
                .path(CHEVRON_DOWN_ICON)
                .size(px(10.0))
                .text_color(rgb(color::fg_kbd())),
        )
}

/// A setting's chip, lit and checked while it is on.
pub fn toggle_chip(label: &str, on: bool) -> Div {
    let (background, border, text) = if on {
        (
            rgba(color::accent() << 8 | 0x29),
            rgba(color::accent() << 8 | 0x66),
            color::fg_alt(),
        )
    } else {
        (
            rgba(glass::control()),
            rgba(glass::control_border()),
            color::fg_kbd(),
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

/// The block cursor's quad, which a text highlight cannot round; text in it is inverted.
pub fn block_cursor() -> Div {
    div()
        .flex_none()
        .rounded(px(cursor::radius()))
        .bg(rgb(color::fg_main()))
        .text_color(rgb(color::bg_main()))
}

/// `text` with the block cursor over its bytes `at`, the text on either side of it
/// styled by `highlights`.
pub fn text_with_cursor(text: &str, highlights: &Highlights, at: Range<usize>) -> Div {
    let part = |range: Range<usize>| {
        let styled = slice(highlights, range.clone());
        div()
            .flex_none()
            .child(StyledText::new(text[range].to_owned()).with_highlights(styled))
    };
    div()
        .flex()
        .child(part(0..at.start))
        .child(block_cursor().child(text[at.clone()].to_owned()))
        .child(part(at.end..text.len()))
}

/// The character under a block cursor drawn on its own, under the text: clear of any
/// background, like the region's, that would cover the cursor.
pub fn cursor_text_style() -> HighlightStyle {
    HighlightStyle {
        color: Some(hsla(color::bg_main())),
        background_color: Some(transparent_black()),
        ..Default::default()
    }
}

/// A floating surface of glass, showing the editor under it blurred.
pub fn glass_surface(radius: f32) -> Div {
    div()
        .bg(rgba(glass::surface()))
        .glass_blur(glass::SURFACE_BLUR, glass::SURFACE_SATURATION)
        .border_1()
        .border_color(rgba(glass::surface_border()))
        .rounded(px(radius))
        .shadow(vec![
            shadow(glass::shadow(0x80), 1.0, 2.0, 0.0),
            shadow(glass::shadow(0x80), 24.0, 60.0, 0.0),
        ])
}

/// A modal surface: thicker glass than a floating one, lifted further.
pub fn modal_surface() -> Div {
    div()
        .bg(rgba(glass::modal()))
        .glass_blur(glass::MODAL_BLUR, glass::SURFACE_SATURATION)
        .border_1()
        .border_color(rgba(glass::modal_border()))
        .rounded(px(glass::SURFACE_RADIUS))
        .shadow(vec![
            shadow(glass::shadow(0x80), 1.0, 2.0, 0.0),
            shadow(glass::shadow(0x99), 30.0, 80.0, 0.0),
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
        .bg(rgb(color::warning()))
        .shadow(vec![shadow(color::warning() << 8 | 0x2e, 0.0, 0.0, 2.0)])
}

/// A translucent floating bar over the bottom of the window, like the status pill.
pub fn pill() -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(2.0))
        .p(px(3.0))
        .rounded(px(9.0))
        .bg(rgba(glass::pill()))
        .glass_blur(glass::PILL_BLUR, 1.0)
        .border_1()
        .border_color(rgba(glass::pill_border()))
        .shadow(vec![shadow(glass::shadow(0x59), 4.0, 14.0, 0.0)])
        .text_size(px(font::STATUS_SIZE))
        .text_color(rgb(color::fg_faint()))
}

pub fn pill_separator() -> Div {
    div().w(px(1.0)).h(px(14.0)).bg(rgba(glass::ink(0x14)))
}

/// A key cap, e.g. `C-x`.
pub fn kbd(key: &str, selected: bool) -> Div {
    let (background, foreground) = if selected {
        (rgb(color::bg_kbd_selected()), color::fg_main())
    } else {
        (rgba(glass::control()), color::fg_kbd())
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
        .border_color(rgba(glass::control_border()))
        .shadow(vec![shadow(glass::shadow(0x99), 1.0, 0.0, 0.0)])
        .font_family(font::mono())
        .text_size(px(font::SMALL_SIZE))
        .text_color(rgb(foreground))
        .child(key.to_owned())
}
