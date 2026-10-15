use std::mem;
use std::ops::Range;
use std::sync::Arc;

const CHUNK_CAPACITY: usize = 64 * 1024;

/// Append-only storage for inserted text. Full chunks are frozen and shared between
/// snapshots; the open chunk is shared too and copied only when appended to while a
/// snapshot holds it, so cloning costs no copy and a key typed at most one.
#[derive(Clone, Default)]
pub(crate) struct AddBuffer {
    frozen: Vec<Arc<[u8]>>,
    current: Arc<Vec<u8>>,
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
        if Arc::get_mut(&mut self.current).is_none_or(|current| current.capacity() == 0) {
            let mut owned = Vec::with_capacity(CHUNK_CAPACITY);
            owned.extend_from_slice(&self.current);
            self.current = Arc::new(owned);
        }
        let current = Arc::get_mut(&mut self.current).expect("the open chunk was just unshared");
        let start = current.len();
        current.extend_from_slice(text);
        (self.frozen.len() as u32, start..current.len())
    }

    pub fn chunk(&self, index: u32) -> &[u8] {
        self.frozen
            .get(index as usize)
            .map_or(self.current.as_slice(), |chunk| chunk)
    }

    fn freeze_current(&mut self) {
        if !self.current.is_empty() {
            self.frozen
                .push(Arc::unwrap_or_clone(mem::take(&mut self.current)).into());
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
