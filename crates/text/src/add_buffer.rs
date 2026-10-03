use std::mem;
use std::ops::Range;
use std::sync::Arc;

const CHUNK_CAPACITY: usize = 64 * 1024;

/// Append-only storage for inserted text. Full chunks are frozen and shared between
/// snapshots, so cloning copies at most one partly filled chunk.
#[derive(Clone, Default)]
pub(crate) struct AddBuffer {
    frozen: Vec<Arc<[u8]>>,
    current: Vec<u8>,
}

impl AddBuffer {
    /// Stores `text` within a single chunk and returns that chunk and the range inside it.
    pub fn append(&mut self, text: &[u8]) -> (u32, Range<usize>) {
        if self.current.len() + text.len() > CHUNK_CAPACITY {
            self.freeze_current();
        }
        if text.len() > CHUNK_CAPACITY {
            self.frozen.push(text.into());
            return (self.frozen.len() as u32 - 1, 0..text.len());
        }
        if self.current.capacity() == 0 {
            self.current.reserve_exact(CHUNK_CAPACITY);
        }
        let start = self.current.len();
        self.current.extend_from_slice(text);
        (self.frozen.len() as u32, start..self.current.len())
    }

    pub fn chunk(&self, index: u32) -> &[u8] {
        self.frozen
            .get(index as usize)
            .map_or(&self.current, |chunk| chunk)
    }

    fn freeze_current(&mut self) {
        if !self.current.is_empty() {
            self.frozen.push(mem::take(&mut self.current).into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_keep_their_indices_across_freezing() {
        let mut buffer = AddBuffer::default();
        let small = buffer.append(b"abc");
        let large_text = vec![b'x'; CHUNK_CAPACITY + 1];
        let large = buffer.append(&large_text);
        let after = buffer.append(b"def");

        assert_eq!(&buffer.chunk(small.0)[small.1], b"abc");
        assert_eq!(&buffer.chunk(large.0)[large.1], &large_text[..]);
        assert_eq!(&buffer.chunk(after.0)[after.1], b"def");
        assert_ne!(small.0, after.0);
    }
}
