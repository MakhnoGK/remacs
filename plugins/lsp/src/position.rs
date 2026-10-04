use std::path::Path;

use lsp_types::Position;
use remacs_core::Editor;
use remacs_text::{Snapshot, Text};

/// What an LSP `character` counts: UTF-16 code units unless the server agreed to bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Encoding {
    Utf8,
    Utf16,
}

pub(crate) fn to_lsp(text: &Snapshot, offset: usize, encoding: Encoding) -> Position {
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
pub(crate) fn from_lsp(text: &Snapshot, position: Position, encoding: Encoding) -> usize {
    let Some(line) = text.line_range(position.line as usize) else {
        return text.len();
    };
    let character = position.character as usize;
    let offset = match encoding {
        Encoding::Utf8 => (line.start + character).min(line.end),
        Encoding::Utf16 => {
            let line_text = String::from_utf8_lossy(&text.bytes_in(line.clone())).into_owned();
            let mut units = 0;
            line_text
                .char_indices()
                .find(|(_, char)| {
                    units += char.len_utf16();
                    units > character
                })
                .map_or(line.end, |(index, _)| line.start + index)
        }
    };
    if text.is_char_boundary(offset) {
        offset
    } else {
        text.previous_char_boundary(offset)
    }
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
}
