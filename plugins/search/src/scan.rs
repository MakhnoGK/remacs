//! Searching the project's files on background threads, the matching lines collected as
//! they are found.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{self, Read};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use memchr::{memchr, memchr_iter};
use memmap2::Mmap;
use remacs_core::Notifier;
use remacs_text::Snapshot;

use crate::words::Words;

/// Smaller files are read into memory rather than mapped.
const MAP_THRESHOLD: u64 = 32 << 20;
/// A file with a NUL byte this near its start is taken as binary and left out.
const BINARY_PROBE: usize = 8 << 10;
/// Bytes of a line shown; a longer one shows the part around its first match.
const MAX_LABEL: usize = 240;
const CONTEXT_BEFORE_MATCH: usize = 40;
/// The prompt is told of new matches at most this often.
const NOTIFY_EVERY: Duration = Duration::from_millis(40);
const MAX_THREADS: usize = 8;

/// What to search: files relative to `root`, after those with unsaved changes as their
/// buffers have them.
pub struct Job {
    pub root: PathBuf,
    pub files: Arc<Vec<String>>,
    pub unsaved: Vec<(String, Snapshot)>,
    pub words: Words,
    pub max_lines: usize,
}

/// A matching line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// From 0.
    pub line: usize,
    /// Bytes from the line's start to its first match.
    pub column: usize,
    /// The line, or the part of a long one around its first match, trimmed.
    pub label: String,
    pub matches: Vec<Range<usize>>,
}

/// The lines found so far, by the file's place in the list searched.
#[derive(Default)]
pub struct Found {
    pub files: BTreeMap<usize, (String, Vec<Hit>)>,
    pub lines: usize,
    pub done: bool,
    /// Stopped at the most lines wanted.
    pub more: bool,
}

/// A search running in the background; dropping it stops it.
pub struct Scan {
    found: Arc<Mutex<Found>>,
    cancelled: Arc<AtomicBool>,
}

impl Scan {
    pub fn start(job: Job, notifier: Notifier) -> Self {
        let found = Arc::new(Mutex::new(Found::default()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            job,
            found: Arc::clone(&found),
            cancelled: Arc::clone(&cancelled),
            notifier,
            notified: Mutex::new(Instant::now()),
        };
        thread::Builder::new()
            .name("project-search".into())
            .spawn(move || worker.run())
            .expect("spawning the project search thread");
        Self { found, cancelled }
    }

    pub fn found(&self) -> MutexGuard<'_, Found> {
        lock(&self.found)
    }
}

impl Drop for Scan {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

fn lock(found: &Mutex<Found>) -> MutexGuard<'_, Found> {
    found
        .lock()
        .expect("no search thread panics holding the results")
}

struct Worker {
    job: Job,
    found: Arc<Mutex<Found>>,
    cancelled: Arc<AtomicBool>,
    notifier: Notifier,
    notified: Mutex<Instant>,
}

impl Worker {
    /// Files are taken in turn by as many threads as there are cores, up to a few.
    fn run(self) {
        let next = AtomicUsize::new(0);
        let unsaved: HashSet<&str> = self
            .job
            .unsaved
            .iter()
            .map(|(file, _)| file.as_str())
            .collect();
        let threads =
            thread::available_parallelism().map_or(1, |count| count.get().min(MAX_THREADS));
        thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| {
                    // Reused from file to file.
                    let (mut read_buffer, mut lowered) = (Vec::new(), Vec::new());
                    while !self.cancelled.load(Ordering::Relaxed) {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let mapped;
                        let (file, bytes) = match self.job.unsaved.get(index) {
                            Some((file, text)) => {
                                read_buffer = text.bytes_in(0..text.len());
                                (file, &read_buffer[..])
                            }
                            None => {
                                let Some(file) = self.job.files.get(index - self.job.unsaved.len())
                                else {
                                    return;
                                };
                                if unsaved.contains(file.as_str()) {
                                    continue;
                                }
                                let path = self.job.root.join(file);
                                match read(&path, &mut read_buffer) {
                                    Ok(Some(mapping)) => {
                                        mapped = mapping;
                                        (file, &mapped[..])
                                    }
                                    Ok(None) => (file, &read_buffer[..]),
                                    Err(_) => continue,
                                }
                            }
                        };
                        let hits = self.search(bytes, &mut lowered);
                        if !hits.is_empty() {
                            self.add(index, file, hits);
                        }
                    }
                });
            }
        });
        if !self.cancelled.load(Ordering::Relaxed) || lock(&self.found).more {
            lock(&self.found).done = true;
            (self.notifier)();
        }
    }

    fn search(&self, bytes: &[u8], lowered: &mut Vec<u8>) -> Vec<Hit> {
        if memchr(0, &bytes[..bytes.len().min(BINARY_PROBE)]).is_some() {
            return Vec::new();
        }
        let mut hits = Vec::new();
        let (mut line, mut counted) = (0, 0);
        self.job.words.lines(bytes, lowered, |range| {
            line += memchr_iter(b'\n', &bytes[counted..range.start]).count();
            counted = range.start;
            let text = String::from_utf8_lossy(&bytes[range]);
            hits.push(hit(&self.job.words, line, &text));
            hits.len() < self.job.max_lines && !self.cancelled.load(Ordering::Relaxed)
        });
        hits
    }

    /// Adds a file's lines, up to the most wanted, and tells the prompt now and then.
    fn add(&self, index: usize, file: &str, mut hits: Vec<Hit>) {
        let mut found = lock(&self.found);
        let room = self.job.max_lines.saturating_sub(found.lines);
        if hits.len() >= room {
            hits.truncate(room);
            found.more = true;
            self.cancelled.store(true, Ordering::Relaxed);
        }
        found.lines += hits.len();
        if !hits.is_empty() {
            found.files.insert(index, (file.to_owned(), hits));
        }
        drop(found);
        let mut notified = self.notified.lock().expect("never poisoned");
        if notified.elapsed() >= NOTIFY_EVERY {
            *notified = Instant::now();
            (self.notifier)();
        }
    }
}

fn hit(words: &Words, line: usize, text: &str) -> Hit {
    let column = words.ranges_in(text).first().map_or(0, |range| range.start);
    let boundary = |mut offset: usize| {
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    };
    let from = if text.len() > MAX_LABEL {
        boundary(column.saturating_sub(CONTEXT_BEFORE_MATCH))
    } else {
        0
    };
    let to = boundary((from + MAX_LABEL).min(text.len()));
    let label = text[from..to].trim().to_owned();
    Hit {
        line,
        column,
        matches: words.ranges_in(&label),
        label,
    }
}

/// Reads a smaller file into `buffer`, replacing what it held; maps a larger one.
fn read(path: &Path, buffer: &mut Vec<u8>) -> io::Result<Option<Mmap>> {
    let mut file = File::open(path)?;
    if file.metadata()?.len() < MAP_THRESHOLD {
        buffer.clear();
        file.read_to_end(buffer)?;
        return Ok(None);
    }
    // SAFETY: the mapping is read-only and lives only while the file is searched; a file
    // truncated meanwhile by another process is the risk ADR 0001 accepts for large files.
    let mapping = unsafe { Mmap::map(&file)? };
    Ok(Some(mapping))
}
