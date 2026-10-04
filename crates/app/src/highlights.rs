use std::ops::Range;

use gpui::{HighlightStyle, UnderlineStyle, px};
use remacs_core::Face;
use remacs_theme::color;

use crate::widgets::hsla;

const UNDERLINE_THICKNESS: f32 = 1.5;

/// Byte ranges of a line's display text and their styles, sorted and non-overlapping.
pub type Highlights = Vec<(Range<usize>, HighlightStyle)>;

pub fn face_style(face: Face) -> HighlightStyle {
    let foreground = |color| HighlightStyle {
        color: Some(hsla(color)),
        ..Default::default()
    };
    let underline = |color| HighlightStyle {
        underline: Some(UnderlineStyle {
            thickness: px(UNDERLINE_THICKNESS),
            color: Some(hsla(color)),
            wavy: false,
        }),
        ..Default::default()
    };
    match face {
        Face::Keyword => foreground(color::KEYWORD),
        Face::Type => foreground(color::TYPE),
        Face::Function => foreground(color::FUNCTION),
        Face::Constant => foreground(color::CONSTANT),
        Face::Builtin => foreground(color::BUILTIN),
        Face::String => foreground(color::STRING),
        Face::Comment => foreground(color::COMMENT),
        Face::Docstring => foreground(color::DOCSTRING),
        Face::Error => underline(color::ERROR),
        Face::Warning => underline(color::WARNING),
        Face::Info => underline(color::INFO),
    }
}

pub fn region_style() -> HighlightStyle {
    HighlightStyle {
        background_color: Some(hsla(color::BG_SELECTION)),
        ..Default::default()
    }
}

/// Merges layers into one list; later layers draw over earlier ones.
pub fn combine(layers: &[Highlights]) -> Highlights {
    let mut bounds: Vec<usize> = layers
        .iter()
        .flatten()
        .flat_map(|(range, _)| [range.start, range.end])
        .collect();
    bounds.sort_unstable();
    bounds.dedup();
    let mut next_in_layer = vec![0; layers.len()];
    let mut combined: Highlights = Vec::new();
    for segment in bounds.windows(2) {
        let (start, end) = (segment[0], segment[1]);
        let mut style: Option<HighlightStyle> = None;
        for (layer, next) in layers.iter().zip(&mut next_in_layer) {
            while layer
                .get(*next)
                .is_some_and(|(range, _)| range.end <= start)
            {
                *next += 1;
            }
            if let Some((range, layer_style)) = layer.get(*next)
                && range.start <= start
            {
                style = Some(style.map_or(*layer_style, |below| below.highlight(*layer_style)));
            }
        }
        let Some(style) = style else {
            continue;
        };
        match combined.last_mut() {
            Some((range, last)) if range.end == start && *last == style => range.end = end,
            _ => combined.push((start..end, style)),
        }
    }
    combined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_layers_draw_over_earlier_ones() {
        let keyword = face_style(Face::Keyword);
        let region = region_style();
        let combined = combine(&[vec![(0..2, keyword), (4..6, keyword)], vec![(1..5, region)]]);
        let both = keyword.highlight(region);
        assert_eq!(
            combined,
            [
                (0..1, keyword),
                (1..2, both),
                (2..4, region),
                (4..5, both),
                (5..6, keyword)
            ]
        );
    }
}
