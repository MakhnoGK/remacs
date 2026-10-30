use std::ops::Range;

use gpui::{FontStyle, FontWeight, HighlightStyle, UnderlineStyle, px, rgba};
use remacs_core::{ConsoleColor, ConsoleStyle, Face};
use remacs_theme::syntax::{self, Style};
use remacs_theme::{color, glass};

use crate::widgets::hsla;

const UNDERLINE_THICKNESS: f32 = 1.5;
/// How far dim colored text fades toward what it is drawn on.
const CONSOLE_DIM_FADE: f32 = 0.4;

/// Byte ranges of a line's display text and their styles, sorted and non-overlapping.
pub type Highlights = Vec<(Range<usize>, HighlightStyle)>;

pub fn face_style(face: Face) -> HighlightStyle {
    let underline = |color: u32| swatch_style(color << 8 | 0xff, UNDERLINE_THICKNESS);
    let background = |color: u32| HighlightStyle {
        background_color: Some(rgba(color).into()),
        ..Default::default()
    };
    match face {
        Face::Keyword => syntax_style(color::keyword(), syntax::keyword()),
        Face::Type => syntax_style(color::type_name(), syntax::type_name()),
        Face::Function => syntax_style(color::function(), syntax::function()),
        Face::Constant => syntax_style(color::constant(), syntax::constant()),
        Face::Builtin => syntax_style(color::builtin(), syntax::builtin()),
        Face::String => syntax_style(color::string(), syntax::string()),
        Face::Comment => syntax_style(color::comment(), syntax::comment()),
        Face::Docstring => syntax_style(color::docstring(), syntax::docstring()),
        Face::Variable => syntax_style(color::variable(), syntax::variable()),
        Face::Error => underline(color::error()),
        Face::Warning => underline(color::warning()),
        Face::Info => underline(color::info()),
        Face::Highlight => background(color::accent() << 8 | 0x2e),
        Face::SearchMatch => background(color::warning() << 8 | 0x38),
        Face::SearchCurrent => HighlightStyle {
            color: Some(hsla(color::bg_main())),
            ..background(color::warning() << 8 | 0xb3)
        },
        Face::MatchingPair => HighlightStyle {
            color: Some(hsla(color::accent_on_selection())),
            ..background(color::accent() << 8 | 0x40)
        },
        // Over the band of their line, which is the same color.
        Face::Added => background(color::fg_added() << 8 | 0x40),
        Face::Removed => background(color::fg_removed() << 8 | 0x40),
        Face::Console(style) => console_style(style),
        Face::Link => HighlightStyle {
            color: Some(hsla(color::link())),
            ..underline(color::link())
        },
        Face::Swatch(color) => swatch_style(color, UNDERLINE_THICKNESS),
    }
}

/// A line under text naming a color, `0xRRGGBBAA`, in it.
pub fn swatch_style(color: u32, thickness: f32) -> HighlightStyle {
    HighlightStyle {
        underline: Some(UnderlineStyle {
            thickness: px(thickness),
            color: Some(rgba(color).into()),
            wavy: false,
        }),
        ..Default::default()
    }
}

fn syntax_style(color: u32, style: Style) -> HighlightStyle {
    text_style(Some(color), style.bold, style.italic, style.underline)
}

/// Text in `color`, bold, italic and underlined in its color as asked.
fn text_style(color: Option<u32>, bold: bool, italic: bool, underline: bool) -> HighlightStyle {
    HighlightStyle {
        color: color.map(hsla),
        font_weight: bold.then_some(FontWeight::BOLD),
        font_style: italic.then_some(FontStyle::Italic),
        underline: underline.then(|| UnderlineStyle {
            thickness: px(1.0),
            color: None,
            wavy: false,
        }),
        ..Default::default()
    }
}

/// Dim text without a color of its own takes the theme's gray, as terminals do.
fn console_style(style: ConsoleStyle) -> HighlightStyle {
    let color = match (style.color, style.dim) {
        (Some(color), _) => Some(console_color(color)),
        (None, true) => Some(color::ansi(8)),
        (None, false) => None,
    };
    HighlightStyle {
        fade_out: (style.dim && style.color.is_some()).then_some(CONSOLE_DIM_FADE),
        ..text_style(color, style.bold, style.italic, style.underline)
    }
}

/// `0xRRGGBB` of an indexed color as xterm defines the 256: the theme's 16, a 6×6×6
/// cube, then 24 grays.
pub fn console_color(color: ConsoleColor) -> u32 {
    const CUBE_LEVELS: [u32; 6] = [0, 95, 135, 175, 215, 255];
    match color {
        ConsoleColor::Rgb(rgb) => rgb,
        ConsoleColor::Indexed(index @ 0..16) => color::ansi(usize::from(index)),
        ConsoleColor::Indexed(index @ 16..232) => {
            let cube = u32::from(index - 16);
            let level = |step: u32| CUBE_LEVELS[(step % 6) as usize];
            level(cube / 36) << 16 | level(cube / 6) << 8 | level(cube)
        }
        ConsoleColor::Indexed(index) => {
            let gray = 8 + 10 * u32::from(index - 232);
            gray << 16 | gray << 8 | gray
        }
    }
}

pub fn region_style() -> HighlightStyle {
    HighlightStyle {
        background_color: Some(rgba(glass::region()).into()),
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

/// The highlights within the bytes `range`, counted from its start.
pub fn slice(highlights: &Highlights, range: Range<usize>) -> Highlights {
    highlights
        .iter()
        .filter_map(|(highlight, style)| {
            let start = highlight.start.max(range.start);
            let end = highlight.end.min(range.end);
            (start < end).then(|| (start - range.start..end - range.start, *style))
        })
        .collect()
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

    #[test]
    fn slices_cut_highlights_at_their_edges() {
        let region = region_style();
        let keyword = face_style(Face::Keyword);
        let highlights = vec![(0..3, keyword), (5..9, region)];
        assert_eq!(slice(&highlights, 2..7), [(0..1, keyword), (3..5, region)]);
        assert_eq!(slice(&highlights, 3..5), []);
    }
}
