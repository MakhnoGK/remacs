use std::str;

/// Whether bytes are UTF-8, checked as they are read: in pieces that may split a
/// character, and in ranges checked apart, on threads, then joined in order.
#[derive(Default)]
pub(crate) struct Utf8Check {
    /// Continuation bytes the range starts with: the end of a character begun before it,
    /// checked once joined to the range before.
    head: Partial,
    /// The start of a character the bytes so far end inside.
    tail: Partial,
    fed: bool,
    head_done: bool,
    invalid: bool,
}

#[derive(Default, Clone, Copy)]
struct Partial {
    bytes: [u8; 4],
    len: usize,
}

impl Partial {
    fn of(bytes: &[u8]) -> Self {
        let mut partial = Self::default();
        partial.bytes[..bytes.len()].copy_from_slice(bytes);
        partial.len = bytes.len();
        partial
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl Utf8Check {
    /// Checks the next bytes of the range.
    pub fn feed(&mut self, mut bytes: &[u8]) {
        self.fed = true;
        if !self.head_done {
            let head_len = bytes
                .iter()
                .take(3 - self.head.len)
                .take_while(|&&byte| is_continuation(byte))
                .count();
            self.head = Partial::of(&[self.head.as_slice(), &bytes[..head_len]].concat());
            bytes = &bytes[head_len..];
            self.head_done = self.head.len == 3 || !bytes.is_empty();
        }
        self.check(bytes);
    }

    /// Continues with `next`, the check of the range right after this one.
    pub fn join(&mut self, next: Self) {
        if !self.fed {
            *self = next;
            return;
        }
        self.check(next.head.as_slice());
        // A range of fewer than three continuation bytes may leave the character open.
        if next.head_done {
            self.invalid |= self.tail.len > 0 || next.invalid;
            self.tail = next.tail;
        }
    }

    /// Whether the range, read from its start to its end, is UTF-8.
    pub fn is_valid(&self) -> bool {
        !self.invalid && self.head.len == 0 && self.tail.len == 0
    }

    fn check(&mut self, mut bytes: &[u8]) {
        if self.invalid {
            return;
        }
        if self.tail.len > 0 {
            let width = char_width(self.tail.bytes[0]);
            let taken = (width - self.tail.len).min(bytes.len());
            let tail = [self.tail.as_slice(), &bytes[..taken]].concat();
            bytes = &bytes[taken..];
            if tail.len() < width {
                self.tail = Partial::of(&tail);
                return;
            }
            self.invalid = str::from_utf8(&tail).is_err();
            self.tail = Partial::default();
        }
        // SIMD, as the std check makes opening a mapped file 40% slower.
        match simdutf8::compat::from_utf8(bytes) {
            Ok(_) => {}
            // Bytes that may yet begin a character the next ones end.
            Err(error) if error.error_len().is_none() => {
                self.tail = Partial::of(&bytes[error.valid_up_to()..]);
            }
            Err(_) => self.invalid = true,
        }
    }
}

pub(crate) fn is_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

/// Length of the character a valid leading byte starts.
fn char_width(leading: u8) -> usize {
    leading.leading_ones().max(1) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rng;

    /// Splits `bytes` into ranges at random, each checked in random pieces, then joined.
    fn check_split(bytes: &[u8], rng: &mut Rng) -> bool {
        let mut whole = Utf8Check::default();
        let mut start = 0;
        while start < bytes.len() {
            let end = (start + 1 + rng.below(8)).min(bytes.len());
            let mut range = Utf8Check::default();
            let mut piece_start = start;
            while piece_start < end {
                let piece_end = (piece_start + 1 + rng.below(4)).min(end);
                range.feed(&bytes[piece_start..piece_end]);
                piece_start = piece_end;
            }
            whole.join(range);
            start = end;
        }
        whole.is_valid()
    }

    #[test]
    fn split_checks_agree_with_checking_all_at_once() {
        let valid = "aé€😀\n".as_bytes();
        let tricky = [0x80, 0xBF, 0xC3, 0xE2, 0xF0, 0xF4, 0xF5, 0xED, 0xA0, 0xC0];
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut invalid_seen = 0;
        for _ in 0..20_000 {
            let mut bytes = Vec::new();
            for _ in 0..rng.below(12) {
                if rng.below(6) == 0 {
                    bytes.push(tricky[rng.below(tricky.len())]);
                } else {
                    let start = rng.below(valid.len());
                    let end = start + rng.below(valid.len() - start + 1);
                    bytes.extend_from_slice(&valid[start..end]);
                }
            }
            let expected = str::from_utf8(&bytes).is_ok();
            invalid_seen += usize::from(!expected);
            assert_eq!(check_split(&bytes, &mut rng), expected, "{bytes:x?}");
        }
        assert!(invalid_seen > 1000 && invalid_seen < 19_000);
    }
}
