use std::ops::Range;

use crate::encoding::Format;
use crate::piece_tree::PieceTree;
use crate::source::Sources;
use crate::utf8::is_continuation;

/// An immutable view of a text, cheap to clone and safe to read from any thread.
/// Offsets are bytes of UTF-8; lines are separated by `\n`.
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

    /// The format of the file the text was loaded from, which saving writes it in.
    pub fn format(&self) -> Format {
        self.sources.format()
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
        let end = self.line_start(line + 1).map_or(self.len(), |next_start| next_start - 1);
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

    /// The whole text's chunks, last first.
    pub(crate) fn chunks_rev(&self) -> impl Iterator<Item = &[u8]> {
        self.pieces.pieces_rev().map(|piece| self.sources.bytes(piece))
    }

    /// Whether both are the same revision, which implies equal contents; O(1).
    pub fn is_same(&self, other: &Snapshot) -> bool {
        self.pieces.is_same(&other.pieces)
    }

    /// Line and byte column of `offset`.
    pub fn position(&self, offset: usize) -> (usize, usize) {
        let line = self.line_of(offset);
        (line, offset - self.line_start(line).unwrap_or(0))
    }

    pub fn bytes_in(&self, range: Range<usize>) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(range.len().min(self.len()));
        for chunk in self.chunks(range) {
            bytes.extend_from_slice(chunk);
        }
        bytes
    }

    pub fn is_char_boundary(&self, offset: usize) -> bool {
        let starts_char = |chunk: &[u8]| !is_continuation(chunk[0]);
        let first_chunk = || self.chunks(offset..offset + 1).next();
        offset == 0 || offset == self.len() || first_chunk().is_some_and(starts_char)
    }

    pub fn next_char_boundary(&self, offset: usize) -> usize {
        (offset + 1..self.len())
            .find(|&candidate| self.is_char_boundary(candidate))
            .unwrap_or(self.len())
    }

    pub fn previous_char_boundary(&self, offset: usize) -> usize {
        (1..offset).rev().find(|&candidate| self.is_char_boundary(candidate)).unwrap_or(0)
    }

    /// Characters between the start of `offset`'s line and `offset`.
    pub fn char_column(&self, offset: usize) -> usize {
        let line_start = self.line_start(self.line_of(offset)).unwrap_or(0);
        self.chunks(line_start..offset).flatten().filter(|&&byte| !is_continuation(byte)).count()
    }

    /// Offset of the `column`-th character of `line`, clamped to the line's end.
    pub fn offset_at_char_column(&self, line: usize, column: usize) -> usize {
        let Some(range) = self.line_range(line) else {
            return self.len();
        };
        let bytes = (range.start..).zip(self.chunks(range.clone()).flatten());
        let mut char_starts = bytes.filter(|&(_, &byte)| !is_continuation(byte));
        char_starts.nth(column).map_or(range.end, |(offset, _)| offset)
    }
}
