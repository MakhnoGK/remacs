use std::ops::Range;

use crate::newline;
use crate::original::Original;
use crate::piece::{Piece, SourceKind};

pub(crate) trait Source {
    fn bytes(&self) -> &[u8];

    fn newlines_in(&self, range: Range<usize>) -> usize {
        newline::count(&self.bytes()[range])
    }

    fn nth_newline_in(&self, range: Range<usize>, rank: usize) -> Option<usize> {
        let start = range.start;
        newline::nth(&self.bytes()[range], rank).map(|offset| start + offset)
    }
}

/// Append-only: pieces keep pointing at bytes once written.
#[derive(Default)]
struct AddBuffer(Vec<u8>);

impl Source for AddBuffer {
    fn bytes(&self) -> &[u8] {
        &self.0
    }
}

pub(crate) struct Sources {
    original: Original,
    added: AddBuffer,
}

impl Sources {
    pub fn new(original: Original) -> Self {
        Self {
            original,
            added: AddBuffer::default(),
        }
    }

    pub fn original_piece(&self) -> Option<Piece> {
        let len = self.original.bytes().len();
        (len > 0).then(|| self.piece(SourceKind::Original, 0..len))
    }

    pub fn append(&mut self, text: &[u8]) -> Piece {
        let start = self.added.0.len();
        self.added.0.extend_from_slice(text);
        self.piece(SourceKind::Added, start..start + text.len())
    }

    pub fn bytes(&self, piece: &Piece) -> &[u8] {
        &self.source(piece.source).bytes()[piece.range()]
    }

    pub fn split(&self, piece: &Piece, offset_in_piece: usize) -> (Piece, Piece) {
        let middle = piece.start + offset_in_piece;
        let left = self.piece(piece.source, piece.start..middle);
        let right = Piece {
            start: middle,
            len: piece.len - offset_in_piece,
            newlines: piece.newlines - left.newlines,
            ..*piece
        };
        (left, right)
    }

    pub fn newlines_in_prefix(&self, piece: &Piece, prefix_len: usize) -> usize {
        self.source(piece.source)
            .newlines_in(piece.start..piece.start + prefix_len)
    }

    pub fn nth_newline_in(&self, piece: &Piece, rank: usize) -> usize {
        self.source(piece.source)
            .nth_newline_in(piece.range(), rank)
            .expect("piece newline count matches its bytes")
            - piece.start
    }

    fn piece(&self, source: SourceKind, range: Range<usize>) -> Piece {
        let newlines = self.source(source).newlines_in(range.clone());
        Piece {
            source,
            start: range.start,
            len: range.len(),
            newlines,
        }
    }

    fn source(&self, kind: SourceKind) -> &dyn Source {
        match kind {
            SourceKind::Original => &self.original,
            SourceKind::Added => &self.added,
        }
    }
}
