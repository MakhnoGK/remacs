use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek};
use std::ops::{Deref, Range};
use std::os::unix::fs::FileExt;
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};

use encoding_rs::{Encoding, UTF_8, WINDOWS_1252};
use memmap2::Mmap;

use crate::encoding::{
    BINARY_SNIFF_LEN, DETECTION_SAMPLE_LEN, Format, Stop, decode, detect, first_line_ends_in_crlf,
    is_binary, utf16_byte_order_mark,
};
use crate::line_index::LineIndex;

/// Smaller files are read into memory: cheap, and immune to the file changing on disk.
const MAP_THRESHOLD: usize = 32 * 1024 * 1024;

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
    /// Reads `file` in `encoding`, or the one its bytes look like when `None`. A binary
    /// file is not read. UTF-8 with LF line ends is kept as it is; anything else is
    /// decoded a chunk at a time, into memory below [`MAP_THRESHOLD`] and above it into a
    /// temporary file that is mapped, so even a huge file costs memory only for the parts
    /// of it read.
    pub fn load(file: &File, encoding: Option<&'static Encoding>) -> io::Result<Self> {
        let file_len = file.metadata()?.len() as usize;
        let small = if file_len < MAP_THRESHOLD {
            let mut bytes = Vec::with_capacity(file_len);
            (&mut &*file).read_to_end(&mut bytes)?;
            Some(bytes)
        } else {
            None
        };
        let head_len = file_len.min(BINARY_SNIFF_LEN);
        let mut head = vec![0; head_len];
        match &small {
            Some(bytes) => head.copy_from_slice(&bytes[..head_len]),
            None => file.read_exact_at(&mut head, 0)?,
        }
        let byte_order_mark = utf16_byte_order_mark(&head);
        let sniffed = encoding.or(byte_order_mark);
        if sniffed.is_none() && is_binary(&head) {
            return Ok(Self::binary(file_len));
        }
        if sniffed.is_none_or(|encoding| encoding == UTF_8) {
            if let Some(plain) = Self::plain(file, file_len, small)? {
                if first_line_ends_in_crlf(&head)
                    && let Ok(without_cr) = Self::decode(file, file_len, Format::crlf())?
                {
                    return Ok(without_cr);
                }
                return Ok(plain);
            }
            if encoding.is_some() {
                return Err(not_in(UTF_8));
            }
        }
        let sniffed = match sniffed {
            Some(sniffed) => sniffed,
            None => {
                let mut sample = vec![0; file_len.min(DETECTION_SAMPLE_LEN)];
                file.read_exact_at(&mut sample, 0)?;
                detect(&sample, sample.len() == file_len)
            }
        };
        let mut format = Format {
            encoding: sniffed,
            byte_order_mark: byte_order_mark == Some(sniffed),
            ..Format::crlf()
        };
        loop {
            match Self::decode(file, file_len, format)? {
                Ok(original) => return Ok(original),
                Err(Stop::LoneLf) => format.crlf = false,
                Err(Stop::Malformed) if encoding.is_some() => return Err(not_in(format.encoding)),
                // Every byte is a character in windows-1252, so this is the last try.
                Err(Stop::Malformed)
                    if format.encoding != WINDOWS_1252 && !format.byte_order_mark =>
                {
                    format.encoding = WINDOWS_1252;
                }
                Err(Stop::Malformed) => return Ok(Self::binary(file_len)),
            }
        }
    }

    /// The file as it is when it is UTF-8: its `small` bytes, else mapped.
    fn plain(file: &File, file_len: usize, small: Option<Vec<u8>>) -> io::Result<Option<Self>> {
        if let Some(bytes) = small {
            let is_utf8 = simdutf8::basic::from_utf8(&bytes).is_ok();
            return Ok(is_utf8.then(|| Self::from(bytes)));
        }
        // SAFETY: the mapping is read-only. Another process may still truncate the file in
        // place while it is mapped; ADR 0001 records that risk for large files.
        let mapping = unsafe { Mmap::map(file)? };
        let (index, is_utf8) = LineIndex::from_file(file, file_len)?;
        Ok(is_utf8.then(|| Self {
            storage: Storage::Mapped(mapping),
            index,
            format: Format::default(),
        }))
    }

    /// The text of `file` read in `format`, whose `crlf` asks to drop the CR of each CR LF.
    fn decode(file: &File, file_len: usize, format: Format) -> io::Result<Result<Self, Stop>> {
        let mut input = file;
        input.rewind()?;
        let with = |crlf| Format { crlf, ..format };
        if file_len < MAP_THRESHOLD {
            let mut text = Vec::with_capacity(file_len);
            return Ok(decode(input, format, &mut text)?
                .map(|crlf| Self { format: with(crlf), ..Self::from(text) }));
        }
        let mut output = BufWriter::with_capacity(1 << 20, temporary_file()?);
        let crlf = match decode(input, format, &mut output)? {
            Ok(crlf) => crlf,
            Err(stop) => return Ok(Err(stop)),
        };
        let text_file = output.into_inner().map_err(io::IntoInnerError::into_error)?;
        let text_len = text_file.metadata()?.len() as usize;
        // SAFETY: nothing else can change the file: its name was removed when it was made.
        let mapping = unsafe { Mmap::map(&text_file)? };
        let (index, _) = LineIndex::from_file(&text_file, text_len)?;
        Ok(Ok(Self { storage: Storage::Mapped(mapping), index, format: with(crlf) }))
    }

    /// A notice in place of the file's bytes, which aren't read.
    fn binary(file_len: usize) -> Self {
        let notice = format!("Binary file of {file_len} bytes: not shown.\n");
        Self {
            format: Format { binary: true, ..Format::default() },
            ..Self::from(notice.into_bytes())
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
        self.index.nth_newline(&self.storage, rank_in_file).filter(|&offset| offset < range.end)
    }
}

fn not_in(encoding: &'static Encoding) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("The file isn't in {}", encoding.name()))
}

/// A file without a name, to hold a decoded text: its pages can leave memory for the disk,
/// which a copy on the heap can't.
// shortcut: the decoded text takes its size on the temporary directory's disk (a 4 GB
// windows-1251 file, ~7 GB) and opening fails without the room; decode lazily if it bites.
fn temporary_file() -> io::Result<File> {
    static MADE: AtomicUsize = AtomicUsize::new(0);
    let path = env::temp_dir().join(format!(
        "remacs-decoded-{}-{}",
        process::id(),
        MADE.fetch_add(1, Ordering::Relaxed)
    ));
    let file = OpenOptions::new().read(true).write(true).create_new(true).open(&path)?;
    fs::remove_file(&path)?;
    Ok(file)
}

impl From<Vec<u8>> for Original {
    fn from(bytes: Vec<u8>) -> Self {
        let index = LineIndex::from_bytes(&bytes);
        Self { storage: Storage::Owned(bytes), index, format: Format::default() }
    }
}
