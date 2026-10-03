use std::fs::File;
use std::io;
use std::ops::{Deref, Range};
use std::path::Path;

use memmap2::Mmap;

use crate::line_index::LineIndex;
use crate::source::Source;

enum Storage {
    Mapped(Mmap),
    Owned(Vec<u8>),
}

impl Deref for Storage {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Storage::Mapped(mapping) => mapping,
            Storage::Owned(bytes) => bytes,
        }
    }
}

/// The text as it was loaded; never mutated, so files are mapped instead of read.
pub(crate) struct Original {
    storage: Storage,
    index: LineIndex,
}

impl Original {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        if file.metadata()?.len() == 0 {
            return Ok(Self::from(Vec::new()));
        }
        // SAFETY: the mapping is read-only. Another process may still modify or truncate
        // the file while it is mapped; detecting that is tracked in ADR 0001.
        let mapping = unsafe { Mmap::map(&file)? };
        let index = LineIndex::from_file(&file, mapping.len())?;
        Ok(Self {
            storage: Storage::Mapped(mapping),
            index,
        })
    }
}

impl From<Vec<u8>> for Original {
    fn from(bytes: Vec<u8>) -> Self {
        let index = LineIndex::from_bytes(&bytes);
        Self {
            storage: Storage::Owned(bytes),
            index,
        }
    }
}

impl Source for Original {
    fn bytes(&self) -> &[u8] {
        &self.storage
    }

    fn newlines_in(&self, range: Range<usize>) -> usize {
        self.index.newlines_before(&self.storage, range.end)
            - self.index.newlines_before(&self.storage, range.start)
    }

    fn nth_newline_in(&self, range: Range<usize>, rank: usize) -> Option<usize> {
        let rank_in_file = self.index.newlines_before(&self.storage, range.start) + rank;
        self.index
            .nth_newline(&self.storage, rank_in_file)
            .filter(|&offset| offset < range.end)
    }
}
