use std::ops::Range;
use std::sync::Arc;

use crate::add_buffer::AddBuffer;
use crate::encoding::Format;
use crate::newline;
use crate::original::Original;
use crate::piece::{Piece, SourceKind};

/// The bytes pieces point into. Cloning is cheap: the original is shared and the add
/// buffer copies at most one chunk.
#[derive(Clone)]
pub(crate) struct Sources {
    original: Arc<Original>,
    added: AddBuffer,
}

impl Sources {
    pub fn new(original: Original) -> Self {
        Self { original: Arc::new(original), added: AddBuffer::default() }
    }

    pub fn format(&self) -> Format {
        self.original.format
    }

    pub fn original_piece(&self) -> Option<Piece> {
        let len = self.original.bytes().len();
        (len > 0).then(|| self.piece(SourceKind::Original, 0..len))
    }

    pub fn append(&mut self, text: &[u8]) -> Piece {
        let (chunk, range) = self.added.append(text);
        self.piece(SourceKind::Added { chunk }, range)
    }

    pub fn bytes(&self, piece: &Piece) -> &[u8] {
        &self.source_bytes(piece.source)[piece.range()]
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
        self.newlines_in(piece.source, piece.start..piece.start + prefix_len)
    }

    /// Offset within `piece` of its `rank`-th newline.
    pub fn nth_newline_in(&self, piece: &Piece, rank: usize) -> usize {
        let offset_in_piece = match piece.source {
            SourceKind::Original => {
                self.original.nth_newline_in(piece.range(), rank).map(|offset| offset - piece.start)
            }
            SourceKind::Added { .. } => newline::nth(self.bytes(piece), rank),
        };
        offset_in_piece.expect("piece newline count matches its bytes")
    }

    fn piece(&self, source: SourceKind, range: Range<usize>) -> Piece {
        Piece {
            source,
            start: range.start,
            len: range.len(),
            newlines: self.newlines_in(source, range),
        }
    }

    fn newlines_in(&self, source: SourceKind, range: Range<usize>) -> usize {
        match source {
            SourceKind::Original => self.original.newlines_in(range),
            SourceKind::Added { .. } => newline::count(&self.source_bytes(source)[range]),
        }
    }

    fn source_bytes(&self, source: SourceKind) -> &[u8] {
        match source {
            SourceKind::Original => self.original.bytes(),
            SourceKind::Added { chunk } => self.added.chunk(chunk),
        }
    }
}
