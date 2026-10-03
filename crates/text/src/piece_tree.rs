use std::ops::Range;

use crate::piece::{Piece, SourceKind, Summary};
use crate::source::Sources;

const MAX_PIECES_PER_LEAF: usize = 64;

struct Leaf {
    pieces: Vec<Piece>,
    summary: Summary,
}

impl Leaf {
    fn new(pieces: Vec<Piece>) -> Self {
        let mut leaf = Self {
            pieces,
            summary: Summary::default(),
        };
        leaf.update_summary();
        leaf
    }

    fn update_summary(&mut self) {
        self.summary = self.pieces.iter().map(Piece::summary).sum();
    }
}

#[derive(Clone, Copy)]
struct Position {
    leaf: usize,
    piece: usize,
}

/// Pieces in document order, grouped into leaves with cached summaries so seeks skip
/// whole leaves. Two levels are enough for the spike; a B-tree replaces it in M1.
pub(crate) struct PieceTree {
    leaves: Vec<Leaf>,
    summary: Summary,
}

impl PieceTree {
    pub fn new(initial: Option<Piece>) -> Self {
        let leaf = Leaf::new(initial.into_iter().collect());
        let summary = leaf.summary;
        Self {
            leaves: vec![leaf],
            summary,
        }
    }

    pub fn summary(&self) -> Summary {
        self.summary
    }

    pub fn insert(&mut self, offset: usize, piece: Piece, sources: &Sources) {
        let position = self.split_at(offset, sources);
        if !self.try_extend_previous(position, &piece) {
            self.leaves[position.leaf]
                .pieces
                .insert(position.piece, piece);
        }
        self.rebalance(position.leaf..position.leaf + 1);
    }

    pub fn remove(&mut self, range: Range<usize>, sources: &Sources) {
        if range.is_empty() {
            return;
        }
        let start = self.split_at(range.start, sources);
        let end = self.split_at(range.end, sources);
        if start.leaf == end.leaf {
            self.leaves[start.leaf].pieces.drain(start.piece..end.piece);
        } else {
            self.leaves[end.leaf].pieces.drain(..end.piece);
            self.leaves[start.leaf].pieces.truncate(start.piece);
            self.leaves.drain(start.leaf + 1..end.leaf);
        }
        self.rebalance(start.leaf..start.leaf + 2);
    }

    pub fn pieces_from(&self, offset: usize) -> impl Iterator<Item = (usize, &Piece)> {
        let (position, before) = self
            .seek(|end| offset < end.len)
            .unwrap_or((self.end_position(), self.summary));
        let first_leaf_rest = self.leaves[position.leaf].pieces[position.piece..].iter();
        let later_leaves = self.leaves[position.leaf + 1..]
            .iter()
            .flat_map(|leaf| &leaf.pieces);
        first_leaf_rest
            .chain(later_leaves)
            .scan(before.len, |piece_start, piece| {
                let start = *piece_start;
                *piece_start += piece.len;
                Some((start, piece))
            })
    }

    /// The piece holding the `rank`-th newline: its document offset and the rank within it.
    pub fn find_newline(&self, rank: usize) -> Option<(usize, &Piece, usize)> {
        let (position, before) = self.seek(|end| rank < end.newlines)?;
        Some((before.len, self.piece(position), rank - before.newlines))
    }

    pub fn newlines_before(&self, offset: usize, sources: &Sources) -> usize {
        match self.seek(|end| offset < end.len) {
            Some((position, before)) => {
                before.newlines
                    + sources.newlines_in_prefix(self.piece(position), offset - before.len)
            }
            None => self.summary.newlines,
        }
    }

    /// First piece whose cumulative end summary satisfies `is_past`, with the summary before it.
    fn seek(&self, is_past: impl Fn(Summary) -> bool) -> Option<(Position, Summary)> {
        let mut before = Summary::default();
        for (leaf_index, leaf) in self.leaves.iter().enumerate() {
            if !is_past(before + leaf.summary) {
                before = before + leaf.summary;
                continue;
            }
            for (piece_index, piece) in leaf.pieces.iter().enumerate() {
                if is_past(before + piece.summary()) {
                    return Some((
                        Position {
                            leaf: leaf_index,
                            piece: piece_index,
                        },
                        before,
                    ));
                }
                before = before + piece.summary();
            }
        }
        None
    }

    /// Ensures a piece boundary at `offset` and returns the position of the piece starting there.
    fn split_at(&mut self, offset: usize, sources: &Sources) -> Position {
        assert!(
            offset <= self.summary.len,
            "offset {offset} past end {}",
            self.summary.len
        );
        let Some((position, before)) = self.seek(|end| offset < end.len) else {
            return self.end_position();
        };
        let offset_in_piece = offset - before.len;
        if offset_in_piece == 0 {
            return position;
        }
        let pieces = &mut self.leaves[position.leaf].pieces;
        let (left, right) = sources.split(&pieces[position.piece], offset_in_piece);
        pieces[position.piece] = left;
        pieces.insert(position.piece + 1, right);
        Position {
            piece: position.piece + 1,
            ..position
        }
    }

    /// Typing appends to the add buffer right after the previous insert; grow that piece
    /// instead of adding one piece per keystroke.
    fn try_extend_previous(&mut self, position: Position, piece: &Piece) -> bool {
        let Some(previous_index) = position.piece.checked_sub(1) else {
            return false;
        };
        let previous = &mut self.leaves[position.leaf].pieces[previous_index];
        let contiguous = previous.source == SourceKind::Added
            && piece.source == SourceKind::Added
            && previous.end() == piece.start;
        if contiguous {
            previous.len += piece.len;
            previous.newlines += piece.newlines;
        }
        contiguous
    }

    fn rebalance(&mut self, touched_leaves: Range<usize>) {
        let end = touched_leaves.end.min(self.leaves.len());
        for index in (touched_leaves.start..end).rev() {
            if self.leaves[index].pieces.is_empty() && self.leaves.len() > 1 {
                self.leaves.remove(index);
                continue;
            }
            if self.leaves[index].pieces.len() > MAX_PIECES_PER_LEAF {
                let upper_half = self.leaves[index].pieces.split_off(MAX_PIECES_PER_LEAF / 2);
                self.leaves.insert(index + 1, Leaf::new(upper_half));
            }
            self.leaves[index].update_summary();
        }
        self.summary = self.leaves.iter().map(|leaf| leaf.summary).sum();
    }

    fn piece(&self, position: Position) -> &Piece {
        &self.leaves[position.leaf].pieces[position.piece]
    }

    fn end_position(&self) -> Position {
        let leaf = self.leaves.len() - 1;
        Position {
            leaf,
            piece: self.leaves[leaf].pieces.len(),
        }
    }
}
