use std::iter::Rev;
use std::mem;
use std::ops::Range;
use std::slice;
use std::sync::Arc;

use crate::piece::{Piece, Summarized, Summary};
use crate::source::Sources;

const MAX_ITEMS: usize = 32;
const MIN_ITEMS: usize = MAX_ITEMS / 2;

/// A persistent B-tree of pieces in document order. Nodes are shared between clones and
/// copied only along the edited path, so a clone is O(1) and an edit O(log n).
#[derive(Clone)]
pub(crate) struct PieceTree {
    root: Arc<Node>,
}

#[derive(Clone)]
struct Node {
    summary: Summary,
    items: Items,
}

#[derive(Clone)]
enum Items {
    Leaf(Vec<Piece>),
    Internal(Vec<Arc<Node>>),
}

impl PieceTree {
    pub fn new(initial: Option<Piece>) -> Self {
        Self {
            root: Node::new(Items::Leaf(initial.into_iter().collect())),
        }
    }

    pub fn summary(&self) -> Summary {
        self.root.summary
    }

    /// Every edit makes a new root, so equal roots mean the same revision.
    pub fn is_same(&self, other: &PieceTree) -> bool {
        Arc::ptr_eq(&self.root, &other.root)
    }

    pub fn insert(&mut self, offset: usize, piece: Piece, sources: &Sources) {
        self.replace(offset..offset, Some(piece), sources);
    }

    pub fn remove(&mut self, range: Range<usize>, sources: &Sources) {
        if !range.is_empty() {
            self.replace(range, None, sources);
        }
    }

    pub fn pieces_from(&self, offset: usize) -> Pieces<'_> {
        Pieces::starting_at(&self.root, offset)
    }

    /// Pieces from the last to the first.
    pub fn pieces_rev(&self) -> PiecesRev<'_> {
        let mut pieces = PiecesRev {
            ancestors: Vec::new(),
            leaf: [].iter().rev(),
        };
        pieces.descend_rightmost(&self.root);
        pieces
    }

    /// The piece holding the `rank`-th newline: its document offset and the rank within it.
    pub fn find_newline(&self, rank: usize) -> Option<(usize, &Piece, usize)> {
        let (piece, before) = self.seek(|end| rank < end.newlines)?;
        Some((before.len, piece, rank - before.newlines))
    }

    pub fn newlines_before(&self, offset: usize, sources: &Sources) -> usize {
        match self.seek(|end| offset < end.len) {
            Some((piece, before)) => {
                before.newlines + sources.newlines_in_prefix(piece, offset - before.len)
            }
            None => self.summary().newlines,
        }
    }

    fn replace(&mut self, range: Range<usize>, insertion: Option<Piece>, sources: &Sources) {
        let len = self.summary().len;
        assert!(range.end <= len, "range {range:?} past end {len}");
        let root = mem::replace(&mut self.root, Node::new(Items::Leaf(Vec::new())));
        let mut nodes = replace(root, range, insertion, sources);
        while nodes.len() > 1 {
            nodes = Items::Internal(nodes).into_nodes();
        }
        let mut root = nodes
            .pop()
            .unwrap_or_else(|| Node::new(Items::Leaf(Vec::new())));
        while let Items::Internal(children) = &root.items
            && children.len() == 1
        {
            root = children[0].clone();
        }
        self.root = root;
    }

    /// First piece whose cumulative end summary satisfies `is_past`, with the summary before it.
    fn seek(&self, is_past: impl Fn(Summary) -> bool) -> Option<(&Piece, Summary)> {
        let mut before = Summary::default();
        let mut node = &*self.root;
        loop {
            match &node.items {
                Items::Internal(children) => {
                    node = &children[find_index(children, &mut before, &is_past)?];
                }
                Items::Leaf(pieces) => {
                    let index = find_index(pieces, &mut before, &is_past)?;
                    return Some((&pieces[index], before));
                }
            }
        }
    }
}

impl Node {
    fn new(items: Items) -> Arc<Self> {
        Arc::new(Self {
            summary: items.summary(),
            items,
        })
    }

    fn is_underfull(&self) -> bool {
        self.items.len() < MIN_ITEMS
    }
}

impl Summarized for Arc<Node> {
    fn summary(&self) -> Summary {
        self.summary
    }
}

impl Items {
    fn len(&self) -> usize {
        match self {
            Items::Leaf(pieces) => pieces.len(),
            Items::Internal(children) => children.len(),
        }
    }

    fn summary(&self) -> Summary {
        match self {
            Items::Leaf(pieces) => pieces.iter().map(Summarized::summary).sum(),
            Items::Internal(children) => children.iter().map(Summarized::summary).sum(),
        }
    }

    fn append(&mut self, other: Items) {
        match (self, other) {
            (Items::Leaf(pieces), Items::Leaf(more)) => pieces.extend(more),
            (Items::Internal(children), Items::Internal(more)) => children.extend(more),
            _ => unreachable!("siblings have equal height"),
        }
    }

    /// Nodes of at most `MAX_ITEMS` each, evenly filled; none when there are no items.
    fn into_nodes(self) -> Vec<Arc<Node>> {
        match self {
            Items::Leaf(pieces) => even_chunks(pieces)
                .map(|chunk| Node::new(Items::Leaf(chunk)))
                .collect(),
            Items::Internal(children) => even_chunks(children)
                .map(|chunk| Node::new(Items::Internal(chunk)))
                .collect(),
        }
    }
}

fn even_chunks<T>(items: Vec<T>) -> impl Iterator<Item = Vec<T>> {
    let chunk_count = items.len().div_ceil(MAX_ITEMS);
    let chunk_len = items.len().div_ceil(chunk_count.max(1));
    let mut items = items.into_iter();
    (0..chunk_count).map(move |_| items.by_ref().take(chunk_len).collect())
}

fn find_index<T: Summarized>(
    items: &[T],
    before: &mut Summary,
    is_past: &impl Fn(Summary) -> bool,
) -> Option<usize> {
    for (index, item) in items.iter().enumerate() {
        let after = *before + item.summary();
        if is_past(after) {
            return Some(index);
        }
        *before = after;
    }
    None
}

/// Replaces `range` (relative to `node`) with `insertion` and returns the nodes that
/// take `node`'s place: none if it emptied, two if it overflowed.
fn replace(
    node: Arc<Node>,
    range: Range<usize>,
    insertion: Option<Piece>,
    sources: &Sources,
) -> Vec<Arc<Node>> {
    let items = match Arc::unwrap_or_clone(node).items {
        Items::Leaf(pieces) => Items::Leaf(replace_in_leaf(pieces, range, insertion, sources)),
        Items::Internal(children) => {
            Items::Internal(replace_in_children(children, range, insertion, sources))
        }
    };
    items.into_nodes()
}

fn replace_in_leaf(
    pieces: Vec<Piece>,
    range: Range<usize>,
    insertion: Option<Piece>,
    sources: &Sources,
) -> Vec<Piece> {
    let mut before = Vec::with_capacity(pieces.len() + 2);
    let mut after = Vec::new();
    let mut piece_start = 0;
    for piece in pieces {
        let piece_end = piece_start + piece.len;
        if piece_end <= range.start {
            before.push(piece);
        } else if piece_start >= range.end {
            after.push(piece);
        } else {
            if piece_start < range.start {
                before.push(sources.split(&piece, range.start - piece_start).0);
            }
            if piece_end > range.end {
                after.push(sources.split(&piece, range.end - piece_start).1);
            }
        }
        piece_start = piece_end;
    }
    if let Some(insertion) = insertion {
        match before.last_mut() {
            Some(previous) if insertion.continues(previous) => {
                previous.len += insertion.len;
                previous.newlines += insertion.newlines;
            }
            _ => before.push(insertion),
        }
    }
    before.extend(after);
    before
}

fn replace_in_children(
    mut children: Vec<Arc<Node>>,
    range: Range<usize>,
    mut insertion: Option<Piece>,
    sources: &Sources,
) -> Vec<Arc<Node>> {
    let (affected, mut child_start) = affected_children(&children, &range);
    let first_affected = affected.start;
    let mut replacements = Vec::new();
    for child in children.drain(affected) {
        let child_len = child.summary.len;
        let local = range.start.saturating_sub(child_start).min(child_len)
            ..range.end.saturating_sub(child_start).min(child_len);
        let deleted_whole = insertion.is_none() && local == (0..child_len);
        if !deleted_whole {
            replacements.extend(replace(child, local, insertion.take(), sources));
        }
        child_start += child_len;
    }
    children.splice(first_affected..first_affected, replacements);
    merge_underfull(&mut children);
    children
}

/// Children overlapping `range` (for an insertion point: the first child ending at or
/// after it, so typing extends the piece to its left) and the offset of the first one.
fn affected_children(children: &[Arc<Node>], range: &Range<usize>) -> (Range<usize>, usize) {
    let mut child_start = 0;
    let mut first = None;
    let mut end = 0;
    for (index, child) in children.iter().enumerate() {
        let child_end = child_start + child.summary.len;
        let touched = if range.is_empty() {
            child_end >= range.start
        } else {
            child_end > range.start && child_start < range.end
        };
        if touched {
            first.get_or_insert((index, child_start));
            end = index + 1;
            if range.is_empty() {
                break;
            }
        } else if first.is_some() {
            break;
        }
        child_start = child_end;
    }
    let (first_index, first_start) = first.unwrap_or((children.len(), child_start));
    (first_index..end.max(first_index), first_start)
}

fn merge_underfull(children: &mut Vec<Arc<Node>>) {
    let mut index = 0;
    while index + 1 < children.len() {
        if !children[index].is_underfull() && !children[index + 1].is_underfull() {
            index += 1;
            continue;
        }
        let right = children.remove(index + 1);
        let mut items = Arc::unwrap_or_clone(children.remove(index)).items;
        items.append(Arc::unwrap_or_clone(right).items);
        let merged = items.into_nodes();
        let merged_count = merged.len();
        children.splice(index..index, merged);
        if merged_count > 1 {
            index += 1;
        }
    }
}

pub(crate) struct Pieces<'a> {
    ancestors: Vec<slice::Iter<'a, Arc<Node>>>,
    leaf: slice::Iter<'a, Piece>,
    offset: usize,
}

impl<'a> Pieces<'a> {
    /// Starts at the piece containing `offset`.
    fn starting_at(root: &'a Node, offset: usize) -> Self {
        let mut pieces = Self {
            ancestors: Vec::new(),
            leaf: [].iter(),
            offset: 0,
        };
        let mut before = Summary::default();
        let is_past = |end: Summary| offset < end.len;
        let mut node = root;
        loop {
            match &node.items {
                Items::Internal(children) => {
                    let Some(index) = find_index(children, &mut before, &is_past) else {
                        return pieces;
                    };
                    pieces.ancestors.push(children[index + 1..].iter());
                    node = &children[index];
                }
                Items::Leaf(leaf) => {
                    if let Some(index) = find_index(leaf, &mut before, &is_past) {
                        pieces.leaf = leaf[index..].iter();
                        pieces.offset = before.len;
                    }
                    return pieces;
                }
            }
        }
    }

    fn descend_leftmost(&mut self, mut node: &'a Node) {
        loop {
            match &node.items {
                Items::Internal(children) => {
                    let mut siblings = children.iter();
                    node = siblings.next().expect("internal nodes are non-empty");
                    self.ancestors.push(siblings);
                }
                Items::Leaf(pieces) => {
                    self.leaf = pieces.iter();
                    return;
                }
            }
        }
    }
}

impl<'a> Iterator for Pieces<'a> {
    type Item = (usize, &'a Piece);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(piece) = self.leaf.next() {
                let piece_start = self.offset;
                self.offset += piece.len;
                return Some((piece_start, piece));
            }
            let next_node = loop {
                match self.ancestors.last_mut()?.next() {
                    Some(node) => break node,
                    None => {
                        self.ancestors.pop();
                    }
                }
            };
            self.descend_leftmost(next_node);
        }
    }
}

pub(crate) struct PiecesRev<'a> {
    ancestors: Vec<Rev<slice::Iter<'a, Arc<Node>>>>,
    leaf: Rev<slice::Iter<'a, Piece>>,
}

impl<'a> PiecesRev<'a> {
    fn descend_rightmost(&mut self, mut node: &'a Node) {
        loop {
            match &node.items {
                Items::Internal(children) => {
                    let mut siblings = children.iter().rev();
                    node = siblings.next().expect("internal nodes are non-empty");
                    self.ancestors.push(siblings);
                }
                Items::Leaf(pieces) => {
                    self.leaf = pieces.iter().rev();
                    return;
                }
            }
        }
    }
}

impl<'a> Iterator for PiecesRev<'a> {
    type Item = &'a Piece;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(piece) = self.leaf.next() {
                return Some(piece);
            }
            let next_node = loop {
                match self.ancestors.last_mut()?.next() {
                    Some(node) => break node,
                    None => {
                        self.ancestors.pop();
                    }
                }
            };
            self.descend_rightmost(next_node);
        }
    }
}

#[cfg(test)]
impl PieceTree {
    pub fn assert_invariants(&self) {
        fn height(node: &Node, is_root: bool) -> usize {
            assert_eq!(node.summary, node.items.summary());
            assert!(node.items.len() <= MAX_ITEMS);
            assert!(is_root || !node.is_underfull(), "underfull non-root node");
            match &node.items {
                Items::Leaf(pieces) => {
                    assert!(pieces.iter().all(|piece| piece.len > 0));
                    1
                }
                Items::Internal(children) => {
                    let heights: Vec<usize> =
                        children.iter().map(|child| height(child, false)).collect();
                    assert!(heights.windows(2).all(|pair| pair[0] == pair[1]));
                    heights[0] + 1
                }
            }
        }
        height(&self.root, true);
    }

    pub fn depth(&self) -> usize {
        let mut depth = 1;
        let mut node = &*self.root;
        while let Items::Internal(children) = &node.items {
            node = &children[0];
            depth += 1;
        }
        depth
    }
}
