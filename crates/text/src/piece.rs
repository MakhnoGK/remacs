use std::iter::Sum;
use std::ops::{Add, Range};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceKind {
    Original,
    Added,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Piece {
    pub source: SourceKind,
    pub start: usize,
    pub len: usize,
    pub newlines: usize,
}

impl Piece {
    pub fn range(&self) -> Range<usize> {
        self.start..self.end()
    }

    pub fn end(&self) -> usize {
        self.start + self.len
    }

    pub fn summary(&self) -> Summary {
        Summary {
            len: self.len,
            newlines: self.newlines,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub len: usize,
    pub newlines: usize,
}

impl Add for Summary {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            len: self.len + other.len,
            newlines: self.newlines + other.newlines,
        }
    }
}

impl Sum for Summary {
    fn sum<I: Iterator<Item = Self>>(summaries: I) -> Self {
        summaries.fold(Self::default(), Add::add)
    }
}
