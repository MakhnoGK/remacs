use crate::snapshot::Snapshot;

/// One replacement turning an old text into a new one: bytes `start..old_end` of the old
/// text became bytes `start..new_end` of the new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub old_end: usize,
    pub new_end: usize,
}

impl Snapshot {
    /// The smallest single edit from `old` to `self`, on char boundaries; `None` when the
    /// contents are equal. Chunks both share compare by address, so the cost follows the
    /// pieces rather than the bytes.
    pub fn edit_from(&self, old: &Snapshot) -> Option<Edit> {
        if self.pieces.is_same(&old.pieces) {
            return None;
        }
        let shorter = self.len().min(old.len());
        let prefix = common_len(old.chunks(0..old.len()), self.chunks(0..self.len()), false);
        if prefix == old.len() && prefix == self.len() {
            return None;
        }
        let prefix = prefix.min(shorter);
        let mut suffix =
            common_len(old.chunks_rev(), self.chunks_rev(), true).min(shorter - prefix);
        let start = if self.is_char_boundary(prefix) {
            prefix
        } else {
            self.previous_char_boundary(prefix)
        };
        while suffix > 0 && !old.is_char_boundary(old.len() - suffix) {
            suffix -= 1;
        }
        Some(Edit { start, old_end: old.len() - suffix, new_end: self.len() - suffix })
    }
}

/// Bytes the two chunk streams have in common from their start, or from their end when
/// `from_end` (the streams then yield chunks last first).
fn common_len<'a>(
    mut old: impl Iterator<Item = &'a [u8]>,
    mut new: impl Iterator<Item = &'a [u8]>,
    from_end: bool,
) -> usize {
    let (mut old_chunk, mut new_chunk): (&[u8], &[u8]) = (&[], &[]);
    let mut common = 0;
    loop {
        if old_chunk.is_empty() {
            match old.next() {
                Some(chunk) => old_chunk = chunk,
                None => return common,
            }
        }
        if new_chunk.is_empty() {
            match new.next() {
                Some(chunk) => new_chunk = chunk,
                None => return common,
            }
        }
        let len = old_chunk.len().min(new_chunk.len());
        // The `len` bytes compared now, then the rest.
        let split = |chunk: &'a [u8]| {
            let (head, tail) = chunk.split_at(if from_end { chunk.len() - len } else { len });
            if from_end { (tail, head) } else { (head, tail) }
        };
        let ((old_part, old_rest), (new_part, new_rest)) = (split(old_chunk), split(new_chunk));
        if !std::ptr::eq(old_part, new_part) && old_part != new_part {
            let pairs = old_part.iter().zip(new_part);
            let equal = |(old, new): &(&u8, &u8)| old == new;
            return common
                + if from_end {
                    pairs.rev().take_while(equal).count()
                } else {
                    pairs.take_while(equal).count()
                };
        }
        common += len;
        (old_chunk, new_chunk) = (old_rest, new_rest);
    }
}

#[cfg(test)]
mod tests {
    use crate::{Rng, Text};

    use super::*;

    fn at(start: usize, old_end: usize, new_end: usize) -> Option<Edit> {
        Some(Edit { start, old_end, new_end })
    }

    fn edit(old: &Text, new: &Text) -> Option<(String, String)> {
        let edit = new.edit_from(old)?;
        let text = |text: &Text, end| String::from_utf8(text.bytes_in(edit.start..end)).unwrap();
        Some((text(old, edit.old_end), text(new, edit.new_end)))
    }

    #[test]
    fn finds_the_changed_range() {
        let mut text = Text::from("fn main() {}\n");
        let old = text.snapshot();
        text.insert(11, "x");
        assert_eq!(text.edit_from(&old), at(11, 11, 12));

        let mut new = Text::from("fn main() {}\n");
        new.delete(3..7);
        new.insert(3, "test");
        assert_eq!(edit(&Text::from("fn main() {}\n"), &new), Some(("main".into(), "test".into())));
    }

    #[test]
    fn equal_contents_have_no_edit() {
        let mut text = Text::from("abc");
        let old = text.snapshot();
        text.insert(1, "x");
        text.delete(1..2);
        assert_eq!(text.edit_from(&old), None);
        assert_eq!(text.edit_from(&text.snapshot()), None);
    }

    #[test]
    fn undo_is_an_edit_too() {
        let mut text = Text::from("one 222 three");
        text.delete(4..8);
        text.add_undo_boundary();
        let edited = text.snapshot();
        text.undo();
        assert_eq!(text.edit_from(&edited), at(4, 4, 8));
    }

    #[test]
    fn keeps_chars_whole() {
        let mut text = Text::from("aéb");
        let old = text.snapshot();
        text.delete(1..3);
        text.insert(1, "è");
        let (removed, inserted) = edit(&Text::from("aéb"), &text).unwrap();
        assert_eq!((removed.as_str(), inserted.as_str()), ("é", "è"));
        assert_eq!(text.edit_from(&old), at(1, 3, 3));
    }

    #[test]
    fn edits_rebuild_the_new_text() {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let mut below = |bound| rng.below(bound);
        let mut text = Text::from("abc\nabc\n");
        for _ in 0..2000 {
            let old = text.snapshot();
            let offset = below(text.len() + 1);
            if below(3) == 0 {
                text.delete(offset..(offset + below(5)).min(text.len()));
            } else {
                text.insert(offset, ["a", "b\n", "abc"][below(3)]);
            }
            if below(4) == 0 {
                text.add_undo_boundary();
                text.undo();
            }
            let all = |snapshot: &Snapshot, range| snapshot.bytes_in(range);
            let rebuilt = match text.edit_from(&old) {
                Some(edit) => [
                    all(&old, 0..edit.start),
                    all(&text, edit.start..edit.new_end),
                    all(&old, edit.old_end..old.len()),
                ]
                .concat(),
                None => all(&old, 0..old.len()),
            };
            assert_eq!(rebuilt, all(&text, 0..text.len()));
        }
    }

    #[test]
    fn repeated_text_edits_stay_in_bounds() {
        let mut text = Text::from("aaaa");
        let old = text.snapshot();
        text.insert(2, "aa");
        let edit = text.edit_from(&old).unwrap();
        assert_eq!(edit.old_end - edit.start + 2, edit.new_end - edit.start);
        assert!(edit.old_end <= old.len() && edit.new_end <= text.len());
    }
}
