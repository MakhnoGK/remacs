use std::fs::File;
use std::io;
use std::num::NonZero;
use std::os::unix::fs::FileExt;
use std::thread;

use crate::newline;
use crate::utf8::Utf8Check;

const BLOCK_SIZE: usize = 16 * 1024;
const BLOCKS_PER_READ: usize = 64;

/// Newline counts per fixed-size block, so a 10 GiB file costs ~5 MiB of index instead
/// of one entry per line. Small blocks keep each lookup to a few mapped pages.
pub(crate) struct LineIndex {
    newlines_before_block: Vec<usize>,
}

impl LineIndex {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_block_counts(bytes.chunks(BLOCK_SIZE).map(newline::count))
    }

    /// Reads with `pread` rather than through the mapping, so indexing leaves no
    /// resident pages behind in this process. Also tells whether the file is UTF-8, while
    /// its bytes are read anyway.
    pub fn from_file(file: &File, file_len: usize) -> io::Result<(Self, bool)> {
        let block_count = file_len.div_ceil(BLOCK_SIZE);
        let worker_count = thread::available_parallelism().map_or(1, NonZero::get);
        let blocks_per_worker = block_count.div_ceil(worker_count).max(1);
        let mut block_counts = vec![0; block_count];

        let utf8 = thread::scope(|scope| {
            let workers: Vec<_> = block_counts
                .chunks_mut(blocks_per_worker)
                .enumerate()
                .map(|(worker, counts)| {
                    let first_block = worker * blocks_per_worker;
                    scope.spawn(move || count_blocks(file, file_len, first_block, counts))
                })
                .collect();
            workers.into_iter().try_fold(Utf8Check::default(), |mut utf8, worker| {
                utf8.join(worker.join().expect("line index worker panicked")?);
                io::Result::Ok(utf8)
            })
        })?;

        Ok((Self::from_block_counts(block_counts), utf8.is_valid()))
    }

    fn from_block_counts(block_counts: impl IntoIterator<Item = usize>) -> Self {
        let mut total = 0;
        let newlines_before_block = std::iter::once(0)
            .chain(block_counts.into_iter().map(|count| {
                total += count;
                total
            }))
            .collect();
        Self { newlines_before_block }
    }

    pub fn total(&self) -> usize {
        *self.newlines_before_block.last().unwrap_or(&0)
    }

    pub fn newlines_before(&self, bytes: &[u8], offset: usize) -> usize {
        let block = offset / BLOCK_SIZE;
        self.newlines_before_block[block] + newline::count(&bytes[block * BLOCK_SIZE..offset])
    }

    pub fn nth_newline(&self, bytes: &[u8], rank: usize) -> Option<usize> {
        if rank >= self.total() {
            return None;
        }
        let block = self.newlines_before_block.partition_point(|&before| before <= rank) - 1;
        let block_start = block * BLOCK_SIZE;
        let block_end = (block_start + BLOCK_SIZE).min(bytes.len());
        let rank_in_block = rank - self.newlines_before_block[block];
        newline::nth(&bytes[block_start..block_end], rank_in_block)
            .map(|offset| block_start + offset)
    }
}

fn count_blocks(
    file: &File,
    file_len: usize,
    first_block: usize,
    counts: &mut [usize],
) -> io::Result<Utf8Check> {
    let mut buffer = vec![0; BLOCK_SIZE * BLOCKS_PER_READ];
    let mut utf8 = Utf8Check::default();
    for (batch_index, batch) in counts.chunks_mut(BLOCKS_PER_READ).enumerate() {
        let offset = (first_block + batch_index * BLOCKS_PER_READ) * BLOCK_SIZE;
        let read_len = (batch.len() * BLOCK_SIZE).min(file_len - offset);
        let bytes = &mut buffer[..read_len];
        file.read_exact_at(bytes, offset as u64)?;
        for (count, block) in batch.iter_mut().zip(bytes.chunks(BLOCK_SIZE)) {
            *count = newline::count(block);
        }
        utf8.feed(bytes);
    }
    Ok(utf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        (0..BLOCK_SIZE * 3 + 777).map(|i| if i % 97 == 0 { b'\n' } else { b'x' }).collect()
    }

    #[test]
    fn matches_naive_scan_across_blocks() {
        let bytes = sample();
        let index = LineIndex::from_bytes(&bytes);
        let offsets: Vec<usize> = memchr::memchr_iter(b'\n', &bytes).collect();

        assert_eq!(index.total(), offsets.len());
        for (rank, &offset) in offsets.iter().enumerate() {
            assert_eq!(index.nth_newline(&bytes, rank), Some(offset));
            assert_eq!(index.newlines_before(&bytes, offset), rank);
            assert_eq!(index.newlines_before(&bytes, offset + 1), rank + 1);
        }
        assert_eq!(index.nth_newline(&bytes, offsets.len()), None);
        assert_eq!(index.newlines_before(&bytes, bytes.len()), offsets.len());
    }

    #[test]
    fn file_index_equals_bytes_index() {
        let mut bytes = sample();
        let path = std::env::temp_dir().join(format!("remacs-line-index-{}", std::process::id()));
        let index_file = |bytes: &[u8]| {
            std::fs::write(&path, bytes).unwrap();
            LineIndex::from_file(&File::open(&path).unwrap(), bytes.len()).unwrap()
        };
        let (file_index, utf8) = index_file(&bytes);
        assert!(utf8);
        assert_eq!(
            file_index.newlines_before_block,
            LineIndex::from_bytes(&bytes).newlines_before_block
        );

        // "€" across the block a worker starts at, then a Latin-1 "é" after it.
        let boundary = BLOCK_SIZE * 2;
        bytes[boundary - 1..boundary + 2].copy_from_slice("€".as_bytes());
        assert!(index_file(&bytes).1);
        bytes[boundary + 100] = 0xE9;
        assert!(!index_file(&bytes).1);
        std::fs::remove_file(&path).unwrap();
    }
}
