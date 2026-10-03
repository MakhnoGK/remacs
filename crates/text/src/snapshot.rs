use std::ops::Range;

use crate::piece_tree::PieceTree;
use crate::source::Sources;

/// An immutable view of a text, cheap to clone and safe to read from any thread.
/// Offsets are bytes; lines are separated by `\n`.
#[derive(Clone)]
pub struct Snapshot {
    pub(crate) sources: Sources,
    pub(crate) pieces: PieceTree,
}

impl Snapshot {
    pub fn len(&self) -> usize {
        self.pieces.summary().len
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn line_count(&self) -> usize {
        self.pieces.summary().newlines + 1
    }

    pub fn line_start(&self, line: usize) -> Option<usize> {
        let Some(newline_rank) = line.checked_sub(1) else {
            return Some(0);
        };
        let (piece_start, piece, rank_in_piece) = self.pieces.find_newline(newline_rank)?;
        Some(piece_start + self.sources.nth_newline_in(piece, rank_in_piece) + 1)
    }

    /// Byte range of `line` without its trailing newline.
    pub fn line_range(&self, line: usize) -> Option<Range<usize>> {
        let start = self.line_start(line)?;
        let end = self
            .line_start(line + 1)
            .map_or(self.len(), |next_start| next_start - 1);
        Some(start..end)
    }

    pub fn line_of(&self, offset: usize) -> usize {
        self.pieces.newlines_before(offset, &self.sources)
    }

    pub fn chunks(&self, range: Range<usize>) -> impl Iterator<Item = &[u8]> {
        self.pieces
            .pieces_from(range.start)
            .take_while(move |(piece_start, _)| *piece_start < range.end)
            .map(move |(piece_start, piece)| {
                let from = range.start.saturating_sub(piece_start);
                let to = (range.end - piece_start).min(piece.len);
                &self.sources.bytes(piece)[from..to]
            })
    }

    pub fn bytes_in(&self, range: Range<usize>) -> Vec<u8> {
        self.chunks(range).flatten().copied().collect()
    }

    pub fn is_char_boundary(&self, offset: usize) -> bool {
        offset == 0
            || offset == self.len()
            || self
                .chunks(offset..offset + 1)
                .next()
                .is_some_and(|chunk| !is_utf8_continuation(chunk[0]))
    }
}

fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}
