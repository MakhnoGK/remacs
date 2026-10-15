use std::ops::Range;
use std::path::Path;

use lsp_types::Position;
use remacs_core::Editor;
use remacs_text::{Snapshot, Text};

/// What an LSP `character` counts: UTF-16 code units unless the server agreed to bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16,
}

pub fn to_lsp(text: &Snapshot, offset: usize, encoding: Encoding) -> Position {
    let (line, column) = text.position(offset);
    let character = match encoding {
        Encoding::Utf8 => column,
        Encoding::Utf16 => String::from_utf8_lossy(&text.bytes_in(offset - column..offset))
            .encode_utf16()
            .count(),
    };
    Position::new(line as u32, character as u32)
}

/// Byte offset of `position`, clamped to its line and kept on a character boundary.
pub fn from_lsp(text: &Snapshot, position: Position, encoding: Encoding) -> usize {
    let Some(line) = text.line_range(position.line as usize) else {
        return text.len();
    };
    let character = position.character as usize;
    let offset = match encoding {
        Encoding::Utf8 => (line.start + character).min(line.end),
        Encoding::Utf16 => utf16_offset(text, line.clone(), character),
    };
    if text.is_char_boundary(offset) {
        offset
    } else {
        text.previous_char_boundary(offset)
    }
}

/// Offset in `line` of the character starting after `units` UTF-16 code units, else the
/// line's end. Reads the line in place: minified lines are megabytes, and servers send
/// hundreds of positions on one.
fn utf16_offset(text: &Snapshot, line: Range<usize>, units: usize) -> usize {
    const BLOCK: usize = 64;
    // A UTF-8 lead byte starts a character of one code unit, or two from 4 bytes on.
    let units_of = |byte: u8| usize::from(byte & 0xC0 != 0x80) + usize::from(byte >= 0xF0);
    let mut counted = 0;
    let mut offset = line.start;
    for chunk in text.chunks(line.clone()) {
        let mut rest = chunk;
        while rest.len() >= BLOCK {
            let block_units: usize = rest[..BLOCK].iter().map(|&byte| units_of(byte)).sum();
            if counted + block_units > units {
                break;
            }
            counted += block_units;
            offset += BLOCK;
            rest = &rest[BLOCK..];
        }
        for &byte in rest {
            counted += units_of(byte);
            if counted > units {
                return offset;
            }
            offset += 1;
        }
    }
    line.end
}

/// The text a server's positions in `path` refer to: the buffer visiting it, else the
/// file, since the server sees that.
pub(crate) fn text_of(editor: &Editor, path: &Path) -> Option<Snapshot> {
    let visited = editor
        .buffers()
        .iter()
        .find(|buffer| buffer.path() == Some(path))
        .map(|buffer| buffer.text().snapshot());
    match visited {
        Some(text) => Some(text),
        None => Some(Text::open(path).ok()?.snapshot()),
    }
}

#[cfg(test)]
mod tests {
    use remacs_text::Text;

    use super::*;

    #[test]
    fn columns_count_code_units_of_the_negotiated_encoding() {
        let text = Text::from("fn x() {}\nlet 😀ж = 1;\n");
        let offset = "fn x() {}\nlet 😀ж".len();
        assert_eq!(to_lsp(&text, offset, Encoding::Utf16), Position::new(1, 7));
        assert_eq!(to_lsp(&text, offset, Encoding::Utf8), Position::new(1, 10));
        for encoding in [Encoding::Utf8, Encoding::Utf16] {
            let position = to_lsp(&text, offset, encoding);
            assert_eq!(from_lsp(&text, position, encoding), offset);
        }
        assert_eq!(
            from_lsp(&text, Position::new(1, 99), Encoding::Utf16),
            text.line_range(1).unwrap().end
        );
        assert_eq!(
            from_lsp(&text, Position::new(1, 5), Encoding::Utf8),
            "fn x() {}\nlet ".len()
        );
        assert_eq!(
            from_lsp(&text, Position::new(9, 0), Encoding::Utf8),
            text.len()
        );
    }

    #[test]
    fn utf16_columns_of_a_long_line_land_on_its_characters() {
        let line = "a😀ж".repeat(100);
        let text = Text::from(format!("x\n{line}\n").as_str());
        let start = text.line_range(1).unwrap().start;
        let mut units = 0;
        for (index, character) in line.char_indices() {
            let position = Position::new(1, units as u32);
            assert_eq!(from_lsp(&text, position, Encoding::Utf16), start + index);
            units += character.len_utf16();
        }
    }
}
