use std::ops::Range;

use lsp_types::ColorInformation;
use remacs_text::{Edit, Snapshot};

use crate::position::{Encoding, from_lsp};

/// Bytes of a file naming a color, like `bg-red-500` or `#1e90ff`, and the color,
/// `0xRRGGBBAA`.
pub(crate) type Swatch = (Range<usize>, u32);

/// The colors a server found in `text`, in the order they come in it.
pub(crate) fn swatches(
    found: Vec<ColorInformation>,
    text: &Snapshot,
    encoding: Encoding,
) -> Vec<Swatch> {
    let mut swatches: Vec<_> = found
        .into_iter()
        .map(|information| {
            let start = from_lsp(text, information.range.start, encoding);
            let end = from_lsp(text, information.range.end, encoding);
            (start..end, rgba(information.color))
        })
        .filter(|(range, _)| !range.is_empty())
        .collect();
    swatches.sort_by_key(|(range, _)| range.start);
    swatches
}

fn rgba(color: lsp_types::Color) -> u32 {
    [color.red, color.green, color.blue, color.alpha]
        .into_iter()
        .fold(0, |rgba, channel| {
            rgba << 8 | (channel.clamp(0.0, 1.0) * 255.0).round() as u32
        })
}

/// Keeps `swatches` on their text through `edit`: those it touches are dropped until the
/// server looks again, those after it move with it.
pub(crate) fn shift(swatches: &mut Vec<Swatch>, edit: Edit) {
    swatches.retain_mut(|(range, _)| {
        if range.end <= edit.start {
            return true;
        }
        if range.start < edit.old_end {
            return false;
        }
        *range = range.start - edit.old_end + edit.new_end..range.end - edit.old_end + edit.new_end;
        true
    });
}

#[cfg(test)]
mod tests {
    use remacs_text::Text;
    use serde_json::json;

    use super::*;

    #[test]
    fn colors_are_found_in_order_and_follow_edits() {
        let text = Text::from("a bg-red-500 text-[#1e90ff]").snapshot();
        let found = json!([
            { "range": { "start": { "line": 0, "character": 13 }, "end": { "line": 0, "character": 27 } },
              "color": { "red": 0.1176, "green": 0.5647, "blue": 1, "alpha": 1 } },
            { "range": { "start": { "line": 0, "character": 2 }, "end": { "line": 0, "character": 12 } },
              "color": { "red": 0.98, "green": 0.17, "blue": 0.21, "alpha": 0.5 } },
        ]);
        let found = serde_json::from_value(found).unwrap();
        let mut swatches = swatches(found, &text, Encoding::Utf16);
        assert_eq!(swatches, [(2..12, 0xfa2b3680), (13..27, 0x1e90ffff)]);

        // `a` becomes `abc`: both move by two.
        shift(
            &mut swatches,
            Edit {
                start: 1,
                old_end: 1,
                new_end: 3,
            },
        );
        assert_eq!(swatches, [(4..14, 0xfa2b3680), (15..29, 0x1e90ffff)]);
        // Typing in the first one drops it until the server looks again.
        shift(
            &mut swatches,
            Edit {
                start: 8,
                old_end: 8,
                new_end: 9,
            },
        );
        assert_eq!(swatches, [(16..30, 0x1e90ffff)]);
        // Typing right after the last one keeps it.
        shift(
            &mut swatches,
            Edit {
                start: 30,
                old_end: 30,
                new_end: 31,
            },
        );
        assert_eq!(swatches, [(16..30, 0x1e90ffff)]);
    }
}
