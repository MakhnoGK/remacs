use memchr::memchr_iter;

pub(crate) fn count(bytes: &[u8]) -> usize {
    memchr_iter(b'\n', bytes).count()
}

/// Offset of the `rank`-th newline (0-based) in `bytes`.
pub(crate) fn nth(bytes: &[u8], rank: usize) -> Option<usize> {
    memchr_iter(b'\n', bytes).nth(rank)
}
