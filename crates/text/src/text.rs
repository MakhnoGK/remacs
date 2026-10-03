use std::io;
use std::ops::Range;
use std::path::Path;

use crate::original::Original;
use crate::piece_tree::PieceTree;
use crate::source::Sources;

/// An editable text addressed by byte offsets. Lines are separated by `\n`.
pub struct Text {
    sources: Sources,
    pieces: PieceTree,
}

impl Text {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self::from_original(Original::open(path.as_ref())?))
    }

    fn from_original(original: Original) -> Self {
        let sources = Sources::new(original);
        let pieces = PieceTree::new(sources.original_piece());
        Self { sources, pieces }
    }

    pub fn len(&self) -> usize {
        self.pieces.summary().len
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn line_count(&self) -> usize {
        self.pieces.summary().newlines + 1
    }

    pub fn line_start(&self, line: usize) -> Option<usize> {
        let Some(newline_rank) = line.checked_sub(1) else {
            return Some(0);
        };
        let (piece_start, piece, rank_in_piece) = self.pieces.find_newline(newline_rank)?;
        Some(piece_start + self.sources.nth_newline_in(piece, rank_in_piece) + 1)
    }

    /// Byte range of `line` without its trailing newline.
    pub fn line_range(&self, line: usize) -> Option<Range<usize>> {
        let start = self.line_start(line)?;
        let end = self
            .line_start(line + 1)
            .map_or(self.len(), |next_start| next_start - 1);
        Some(start..end)
    }

    pub fn line_of(&self, offset: usize) -> usize {
        self.pieces.newlines_before(offset, &self.sources)
    }

    pub fn chunks(&self, range: Range<usize>) -> impl Iterator<Item = &[u8]> {
        self.pieces
            .pieces_from(range.start)
            .take_while(move |(piece_start, _)| *piece_start < range.end)
            .map(move |(piece_start, piece)| {
                let from = range.start.saturating_sub(piece_start);
                let to = (range.end - piece_start).min(piece.len);
                &self.sources.bytes(piece)[from..to]
            })
    }

    pub fn bytes_in(&self, range: Range<usize>) -> Vec<u8> {
        self.chunks(range).flatten().copied().collect()
    }

    pub fn insert(&mut self, offset: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let piece = self.sources.append(text.as_bytes());
        self.pieces.insert(offset, piece, &self.sources);
    }

    pub fn delete(&mut self, range: Range<usize>) {
        assert!(
            range.end <= self.len(),
            "range {range:?} past end {}",
            self.len()
        );
        self.pieces.remove(range, &self.sources);
    }
}

impl From<&str> for Text {
    fn from(text: &str) -> Self {
        Self::from_original(Original::from(text.as_bytes().to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// xorshift64: deterministic and dependency-free.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, bound: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % bound.max(1) as u64) as usize
        }
    }

    fn assert_matches(text: &Text, model: &[u8]) {
        assert_eq!(text.bytes_in(0..text.len()), model);
        let model_lines: Vec<&[u8]> = model.split(|&byte| byte == b'\n').collect();
        assert_eq!(text.line_count(), model_lines.len());
        for (line, expected) in model_lines.iter().enumerate() {
            let range = text.line_range(line).unwrap();
            assert_eq!(&text.bytes_in(range.clone()), expected, "line {line}");
            assert_eq!(text.line_of(range.start), line);
            assert_eq!(text.line_of(range.end), line);
        }
        assert_eq!(text.line_start(model_lines.len()), None);
    }

    #[test]
    fn empty_text_has_one_empty_line() {
        let text = Text::from("");
        assert!(text.is_empty());
        assert_eq!(text.line_count(), 1);
        assert_eq!(text.line_range(0), Some(0..0));
    }

    #[test]
    fn random_edits_match_model() {
        const FRAGMENTS: [&str; 5] = ["a", "\n", "fn main() {}\n", "ü", "xyz\n\nq"];
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let initial = "line one\nline two\n\nline four";
        let mut text = Text::from(initial);
        let mut model = initial.as_bytes().to_vec();

        for step in 0..3000 {
            if rng.below(3) == 0 && !model.is_empty() {
                let start = rng.below(model.len());
                let end = (start + rng.below(16)).min(model.len());
                text.delete(start..end);
                model.drain(start..end);
            } else {
                let offset = rng.below(model.len() + 1);
                let fragment = FRAGMENTS[rng.below(FRAGMENTS.len())];
                text.insert(offset, fragment);
                model.splice(offset..offset, fragment.bytes());
            }
            if step % 300 == 0 {
                assert_matches(&text, &model);
            }
        }
        assert_matches(&text, &model);
    }

    #[test]
    fn consecutive_typing_grows_one_piece() {
        let mut text = Text::from("ab");
        for (index, character) in ["x", "y", "z"].into_iter().enumerate() {
            text.insert(1 + index, character);
        }
        assert_eq!(text.bytes_in(0..text.len()), b"axyzb");
        assert_eq!(text.pieces.pieces_from(0).count(), 3);
    }

    #[test]
    fn opened_file_matches_contents() {
        let contents: String = (0..20_000)
            .map(|line| format!("{line}: some text\n"))
            .collect();
        let path = std::env::temp_dir().join(format!("remacs-text-{}", std::process::id()));
        std::fs::write(&path, &contents).unwrap();
        let mut text = Text::open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        let mut model = contents.into_bytes();
        assert_matches(&text, &model);
        text.insert(5, "inserted\n");
        model.splice(5..5, "inserted\n".bytes());
        text.delete(100_000..100_050);
        model.drain(100_000..100_050);
        assert_matches(&text, &model);
    }
}
