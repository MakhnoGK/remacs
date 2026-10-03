use std::fs::File;
use std::io::{self, Read};
use std::ops::{Deref, Range};

use memmap2::Mmap;

use crate::line_index::LineIndex;

/// Smaller files are read into memory: cheap, and immune to the file changing on disk.
const MAP_THRESHOLD: u64 = 32 * 1024 * 1024;

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

/// The text as it was loaded; never mutated.
pub(crate) struct Original {
    storage: Storage,
    index: LineIndex,
}

impl Original {
    pub fn load(mut file: File) -> io::Result<Self> {
        let file_len = file.metadata()?.len();
        if file_len < MAP_THRESHOLD {
            let mut bytes = Vec::with_capacity(file_len as usize);
            file.read_to_end(&mut bytes)?;
            return Ok(Self::from(bytes));
        }
        // SAFETY: the mapping is read-only. Another process may still truncate the file in
        // place while it is mapped; ADR 0001 records that risk for large files.
        let mapping = unsafe { Mmap::map(&file)? };
        let index = LineIndex::from_file(&file, mapping.len())?;
        Ok(Self {
            storage: Storage::Mapped(mapping),
            index,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.storage
    }

    pub fn newlines_in(&self, range: Range<usize>) -> usize {
        self.index.newlines_before(&self.storage, range.end)
            - self.index.newlines_before(&self.storage, range.start)
    }

    pub fn nth_newline_in(&self, range: Range<usize>, rank: usize) -> Option<usize> {
        let rank_in_file = self.index.newlines_before(&self.storage, range.start) + rank;
        self.index
            .nth_newline(&self.storage, rank_in_file)
            .filter(|&offset| offset < range.end)
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
