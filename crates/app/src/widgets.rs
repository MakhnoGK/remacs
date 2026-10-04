use gpui::{
    BoxShadow, Div, HighlightStyle, Hsla, IntoElement, ParentElement, Styled, div, point, px, rgb,
    rgba,
};
use remacs_theme::{color, font};

pub fn hsla(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// The block cursor: the character under it drawn inverted.
pub fn cursor_style() -> HighlightStyle {
    HighlightStyle {
        color: Some(hsla(color::BG_MAIN)),
        background_color: Some(hsla(color::FG_MAIN)),
        ..Default::default()
    }
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

/// A key cap, e.g. `C-x`.
pub fn kbd(key: &str, selected: bool) -> Div {
    let (background, foreground) = if selected {
        (color::BG_KBD_SELECTED, color::FG_MAIN)
    } else {
        (color::BG_KBD, color::FG_KBD)
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(20.0))
        .px(px(6.0))
        .rounded(px(5.0))
        .bg(rgb(background))
        .border_1()
        .border_color(rgba(0xffffff12))
        .shadow(vec![shadow(0x00000099, 1.0, 0.0, 0.0)])
        .font_family(font::MONO)
        .text_size(px(font::SMALL_SIZE))
        .text_color(rgb(foreground))
        .child(key.to_owned())
}
