use std::fs::File;
use std::io::{self, Read};
use std::ops::{Deref, Range};

use memmap2::Mmap;

use crate::encoding::{Format, decode_file};
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

/// The text as it was loaded, always UTF-8 with LF line ends; never mutated.
pub(crate) struct Original {
    storage: Storage,
    index: LineIndex,
    /// The file's, which saving writes the text back in.
    pub format: Format,
}

impl Original {
    /// Files that aren't UTF-8 with LF line ends are decoded into memory; only those that
    /// are can be mapped.
    // shortcut: a large file to decode is held whole, up to twice its size (a 4 GB binary
    // takes ~6 GB); add a size limit or a hex view once big binaries get opened.
    pub fn load(mut file: File) -> io::Result<Self> {
        let file_len = file.metadata()?.len();
        if file_len < MAP_THRESHOLD {
            let mut bytes = Vec::with_capacity(file_len as usize);
            file.read_to_end(&mut bytes)?;
            let (format, text) = decode_file(&bytes, || simdutf8::basic::from_utf8(&bytes).is_ok());
            return Ok(Self::decoded(text.unwrap_or(bytes), format));
        }
        // SAFETY: the mapping is read-only. Another process may still truncate the file in
        // place while it is mapped; ADR 0001 records that risk for large files.
        let mapping = unsafe { Mmap::map(&file)? };
        let (index, is_utf8) = LineIndex::from_file(&file, mapping.len())?;
        Ok(match decode_file(&mapping, || is_utf8) {
            (format, Some(text)) => Self::decoded(text, format),
            (format, None) => Self {
                storage: Storage::Mapped(mapping),
                index,
                format,
            },
        })
    }

    fn decoded(text: Vec<u8>, format: Format) -> Self {
        Self {
            format,
            ..Self::from(text)
        }
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
            format: Format::default(),
        }
    }
}
