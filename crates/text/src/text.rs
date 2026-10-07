use std::fs::File;
use std::io;
use std::ops::{Deref, Range};
use std::path::Path;

use crate::disk::DiskFile;
use crate::history::History;
use crate::original::Original;
use crate::piece_tree::PieceTree;
use crate::snapshot::Snapshot;
use crate::source::Sources;

/// An editable text with undo history, optionally backed by a file.
/// Reading goes through [`Snapshot`], which it dereferences to.
pub struct Text {
    snapshot: Snapshot,
    history: History,
    saved: PieceTree,
    /// Saves so far, which tell a save from undoing back to the saved text.
    saves: u64,
    disk_file: Option<DiskFile>,
}

impl Text {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let file = File::open(path)?;
        let disk_file = DiskFile::opened(path, &file)?;
        let mut text = Self::from_original(Original::load(file)?);
        text.disk_file = Some(disk_file);
        Ok(text)
    }

    fn from_original(original: Original) -> Self {
        let sources = Sources::new(original);
        let pieces = PieceTree::new(sources.original_piece());
        Self {
            saved: pieces.clone(),
            snapshot: Snapshot { sources, pieces },
            history: History::default(),
            saves: 0,
            disk_file: None,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }

    pub fn insert(&mut self, offset: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        self.assert_char_boundary(offset);
        self.history.record(&self.snapshot.pieces, offset);
        let Snapshot { sources, pieces } = &mut self.snapshot;
        let piece = sources.append(text.as_bytes());
        pieces.insert(offset, piece, sources);
    }

    /// Appends output nobody typed, like a program's, to a text nobody edits: it is not
    /// recorded for undo, and an unmodified text stays unmodified.
    pub fn append_output(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let unmodified = !self.is_modified();
        let end = self.snapshot.len();
        let Snapshot { sources, pieces } = &mut self.snapshot;
        let piece = sources.append(text.as_bytes());
        pieces.insert(end, piece, sources);
        if unmodified {
            self.saved = self.snapshot.pieces.clone();
        }
    }

    /// Drops output from `len` on, as a program redrawing its last line does; unrecorded,
    /// like [`Text::append_output`].
    pub fn truncate_output(&mut self, len: usize) {
        let end = self.snapshot.len();
        if len >= end {
            return;
        }
        self.assert_char_boundary(len);
        let unmodified = !self.is_modified();
        let Snapshot { sources, pieces } = &mut self.snapshot;
        pieces.remove(len..end, sources);
        if unmodified {
            self.saved = self.snapshot.pieces.clone();
        }
    }

    pub fn delete(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        self.assert_char_boundary(range.start);
        self.assert_char_boundary(range.end);
        self.history.record(&self.snapshot.pieces, range.start);
        let Snapshot { sources, pieces } = &mut self.snapshot;
        pieces.remove(range, sources);
    }

    /// Closes the current undo group; the command loop calls it after each command.
    pub fn add_undo_boundary(&mut self) {
        self.history.add_boundary();
    }

    /// Returns the offset of the undone edit.
    pub fn undo(&mut self) -> Option<usize> {
        let revision = self.history.undo(&self.snapshot.pieces)?;
        self.snapshot.pieces = revision.pieces;
        Some(revision.offset)
    }

    /// Returns the offset of the redone edit.
    pub fn redo(&mut self) -> Option<usize> {
        let revision = self.history.redo(&self.snapshot.pieces)?;
        self.snapshot.pieces = revision.pieces;
        Some(revision.offset)
    }

    pub fn is_modified(&self) -> bool {
        !self.snapshot.pieces.is_same(&self.saved)
    }

    pub fn path(&self) -> Option<&Path> {
        self.disk_file
            .as_ref()
            .map(|disk_file| disk_file.path.as_path())
    }

    /// Whether the file was replaced or modified by someone else since it was loaded or saved.
    pub fn changed_on_disk(&self) -> io::Result<bool> {
        self.disk_file.as_ref().map_or(Ok(false), DiskFile::changed)
    }

    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .path()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "text has no file"))?
            .to_path_buf();
        self.save_as(path)
    }

    pub fn save_as(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        let chunks = self.snapshot.chunks(0..self.snapshot.len());
        self.disk_file = Some(DiskFile::write(path.as_ref(), chunks)?);
        self.saved = self.snapshot.pieces.clone();
        self.saves += 1;
        Ok(())
    }

    pub fn saves(&self) -> u64 {
        self.saves
    }

    fn assert_char_boundary(&self, offset: usize) {
        assert!(
            self.snapshot.is_char_boundary(offset),
            "offset {offset} is not a char boundary"
        );
    }
}

impl Deref for Text {
    type Target = Snapshot;

    fn deref(&self) -> &Snapshot {
        &self.snapshot
    }
}

impl From<&str> for Text {
    fn from(text: &str) -> Self {
        Self::from_original(Original::from(text.as_bytes().to_vec()))
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::from("")
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

    fn assert_matches(text: &Snapshot, model: &[u8]) {
        text.pieces.assert_invariants();
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

    fn char_boundary_below(model: &[u8], offset: usize) -> usize {
        (0..=offset)
            .rev()
            .find(|&candidate| std::str::from_utf8(&model[..candidate]).is_ok())
            .unwrap()
    }

    fn temporary_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("remacs-{name}-{}", std::process::id()))
    }

    #[test]
    fn empty_text_has_one_empty_line() {
        let text = Text::default();
        assert!(text.is_empty());
        assert_eq!(text.line_count(), 1);
        assert_eq!(text.line_range(0), Some(0..0));
    }

    #[test]
    fn random_edits_match_model_and_undo_to_start() {
        const FRAGMENTS: [&str; 5] = ["a", "\n", "fn main() {}\n", "ü", "xyz\n\nq"];
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let initial = "line one\nline two\n\nline four";
        let mut text = Text::from(initial);
        let mut model = initial.as_bytes().to_vec();
        let mut models = vec![model.clone()];

        for step in 0..5000 {
            if rng.below(3) == 0 && !model.is_empty() {
                let start = char_boundary_below(&model, rng.below(model.len()));
                let end = char_boundary_below(&model, (start + rng.below(16)).min(model.len()));
                text.delete(start..end);
                model.drain(start..end);
            } else {
                let offset = char_boundary_below(&model, rng.below(model.len() + 1));
                let fragment = FRAGMENTS[rng.below(FRAGMENTS.len())];
                text.insert(offset, fragment);
                model.splice(offset..offset, fragment.bytes());
            }
            text.add_undo_boundary();
            if models.last() != Some(&model) {
                models.push(model.clone());
            }
            if step % 500 == 0 {
                assert_matches(&text, &model);
            }
        }
        assert_matches(&text, &model);
        assert!(
            text.pieces.depth() > 2,
            "edits should grow a multi-level tree"
        );

        models.pop();
        while let Some(expected) = models.pop() {
            text.undo().unwrap();
            assert_eq!(text.bytes_in(0..text.len()), expected);
        }
        assert_eq!(text.undo(), None);
        assert!(!text.is_modified());
        text.redo().unwrap();
        assert!(text.is_modified());
    }

    #[test]
    fn typing_is_one_piece_and_one_undo_group() {
        let mut text = Text::from("ab");
        for (index, character) in ["x", "y", "z"].into_iter().enumerate() {
            text.insert(1 + index, character);
        }
        assert_eq!(text.bytes_in(0..text.len()), b"axyzb");
        assert_eq!(text.pieces.pieces_from(0).count(), 3);
        text.undo();
        assert_eq!(text.bytes_in(0..text.len()), b"ab");
    }

    #[test]
    fn output_is_neither_undone_nor_a_modification() {
        let mut text = Text::from("$ ");
        text.append_output("ok\n");
        assert_eq!(text.bytes_in(0..text.len()), b"$ ok\n");
        assert!(!text.is_modified());
        text.append_output("50%");
        text.truncate_output(5);
        text.append_output("100%");
        assert_eq!(text.bytes_in(0..text.len()), b"$ ok\n100%");
        assert!(!text.is_modified());
        assert_eq!(text.undo(), None);
        text.insert(0, "#");
        text.append_output("done\n");
        assert!(text.is_modified());
    }

    #[test]
    fn snapshots_do_not_see_later_edits() {
        let mut text = Text::from("hello");
        let before = text.snapshot();
        text.insert(5, " world");
        let reader = std::thread::spawn(move || before.bytes_in(0..before.len()));
        assert_eq!(reader.join().unwrap(), b"hello");
        assert_eq!(text.bytes_in(0..text.len()), b"hello world");
    }

    #[test]
    #[should_panic(expected = "not a char boundary")]
    fn rejects_offsets_inside_a_char() {
        Text::from("ü").insert(1, "x");
    }

    #[test]
    fn opened_file_matches_contents() {
        let contents: String = (0..20_000)
            .map(|line| format!("{line}: some text\n"))
            .collect();
        let path = temporary_path("open");
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

    #[test]
    fn save_writes_contents_and_tracks_disk_changes() {
        let path = temporary_path("save");
        std::fs::write(&path, "one\n").unwrap();
        let mut text = Text::open(&path).unwrap();
        assert!(!text.changed_on_disk().unwrap());

        text.insert(4, "two\n");
        assert!(text.is_modified());
        text.save().unwrap();
        assert!(!text.is_modified());
        assert!(!text.changed_on_disk().unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        assert_eq!(text.saves(), 1);

        text.add_undo_boundary();
        text.insert(0, "zero\n");
        text.undo();
        assert!(!text.is_modified());
        assert_eq!(text.saves(), 1, "undoing back to the saved text is no save");

        std::fs::write(&path, "changed elsewhere\n").unwrap();
        assert!(text.changed_on_disk().unwrap());
        std::fs::remove_file(&path).unwrap();
    }
}
