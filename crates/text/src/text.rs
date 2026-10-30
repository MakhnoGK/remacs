use std::io;
use std::mem;
use std::ops::{Deref, Range};
use std::path::Path;

use encoding_rs::Encoding;

use crate::diff::Edit;
use crate::disk::{DiskFile, open_regular_file};
use crate::history::{History, Revision};
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
    /// Reads the file at `path` in the encoding its bytes look like.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::open_in(path.as_ref(), None)
    }

    /// Reads the file at `path` in `encoding`, failing if its bytes aren't.
    pub fn open_with_encoding(
        path: impl AsRef<Path>,
        encoding: &'static Encoding,
    ) -> io::Result<Self> {
        Self::open_in(path.as_ref(), Some(encoding))
    }

    fn open_in(path: &Path, encoding: Option<&'static Encoding>) -> io::Result<Self> {
        let file = open_regular_file(path)?;
        let disk_file = DiskFile::opened(path, &file)?;
        let mut text = Self::from_original(Original::load(&file, encoding)?);
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
        self.replace(offset..offset, text);
    }

    /// Appends output nobody typed, like a program's, to a text nobody edits: it is not
    /// recorded for undo, and an unmodified text stays unmodified.
    pub fn append_output(&mut self, text: &str) {
        let end = self.snapshot.len();
        self.replace_unrecorded(end..end, text);
    }

    /// Drops output from `len` on, as a program redrawing its last line does; unrecorded,
    /// like [`Text::append_output`].
    pub fn truncate_output(&mut self, len: usize) {
        let end = self.snapshot.len();
        if len < end {
            self.assert_char_boundary(len);
            self.replace_unrecorded(len..end, "");
        }
    }

    pub fn delete(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        self.assert_char_boundary(range.start);
        self.assert_char_boundary(range.end);
        self.history.record(&self.snapshot.pieces, range.start);
        self.replace(range, "");
    }

    /// Either removes `range` or inserts `text` at its start: the edits above never both.
    fn replace(&mut self, range: Range<usize>, text: &str) {
        let Snapshot { sources, pieces } = &mut self.snapshot;
        if text.is_empty() {
            pieces.remove(range, sources);
        } else {
            let piece = sources.append(text.as_bytes());
            pieces.insert(range.start, piece, sources);
        }
    }

    fn replace_unrecorded(&mut self, range: Range<usize>, text: &str) {
        let unmodified = !self.is_modified();
        self.replace(range, text);
        if unmodified {
            self.saved = self.snapshot.pieces.clone();
        }
    }

    /// Closes the current undo group; the command loop calls it after each command.
    pub fn add_undo_boundary(&mut self) {
        self.history.add_boundary();
    }

    /// Groups undo can revert; an edit that opens a group adds one.
    pub fn undo_depth(&self) -> usize {
        self.history.depth()
    }

    /// Returns the offset of the undone edit.
    pub fn undo(&mut self) -> Option<usize> {
        self.step_history(History::undo)
    }

    /// Returns the offset of the redone edit.
    pub fn redo(&mut self) -> Option<usize> {
        self.step_history(History::redo)
    }

    fn step_history(
        &mut self,
        step: fn(&mut History, &PieceTree) -> Option<Revision>,
    ) -> Option<usize> {
        let revision = step(&mut self.history, &self.snapshot.pieces)?;
        self.snapshot.pieces = revision.pieces;
        Some(revision.offset)
    }

    /// Where each group of edits undo would revert starts in the text now, the latest
    /// first: moved by the edits since, like a marker, onto their start when they replaced
    /// it. Groups that left the text as it was are left out.
    pub fn change_offsets(&self) -> impl Iterator<Item = usize> + '_ {
        let mut after = self.snapshot.clone();
        let mut before = after.clone();
        // Newest first.
        let mut later: Vec<Edit> = Vec::new();
        self.history.states().filter_map(move |state| {
            before.pieces = state.clone();
            let edit = after.edit_from(&before);
            mem::swap(&mut after, &mut before);
            let edit = edit?;
            let offset = later.iter().rev().fold(edit.start, |offset, later| {
                if offset <= later.start {
                    offset
                } else if offset >= later.old_end {
                    offset + later.new_end - later.old_end
                } else {
                    later.start
                }
            });
            later.push(edit);
            Some(offset)
        })
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

    /// Writes the text in the format of the file it was loaded from; fails, leaving the
    /// file as it was, when that can't hold one of its characters or the file is binary.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        let snapshot = &self.snapshot;
        let chunks = snapshot.chunks(0..snapshot.len());
        let disk_file = DiskFile::write(path.as_ref(), |output| {
            snapshot.format().write(chunks, output)
        })?;
        self.disk_file = Some(disk_file);
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
    use crate::Rng;
    use encoding_rs::{UTF_8, WINDOWS_1251, WINDOWS_1252, WINDOWS_1253};
    use std::fs;
    use std::path::PathBuf;

    fn assert_matches(text: &Snapshot, model: &[u8]) {
        text.pieces.assert_invariants();
        assert_eq!(all(text), model);
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

    fn all(text: &Snapshot) -> Vec<u8> {
        text.bytes_in(0..text.len())
    }

    /// A temporary file of `contents`.
    fn file(name: &str, contents: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("remacs-{name}-{}", std::process::id()));
        fs::write(&path, contents).unwrap();
        path
    }

    fn opened(name: &str, contents: &[u8]) -> (PathBuf, Text) {
        let path = file(name, contents);
        let text = Text::open(&path).unwrap();
        (path, text)
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
            assert_eq!(all(&text), expected);
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
        assert_eq!(all(&text), b"axyzb");
        assert_eq!(text.pieces.pieces_from(0).count(), 3);
        text.undo();
        assert_eq!(all(&text), b"ab");
    }

    #[test]
    fn output_is_neither_undone_nor_a_modification() {
        let mut text = Text::from("$ ");
        text.append_output("ok\n");
        assert_eq!(all(&text), b"$ ok\n");
        assert!(!text.is_modified());
        text.append_output("50%");
        text.truncate_output(5);
        text.append_output("100%");
        assert_eq!(all(&text), b"$ ok\n100%");
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
        assert_eq!(all(&text), b"hello world");
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
        let (path, mut text) = opened("open", contents.as_bytes());
        fs::remove_file(&path).unwrap();

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
        let (path, mut text) = opened("save", b"one\n");
        assert!(!text.changed_on_disk().unwrap());

        text.insert(4, "two\n");
        assert!(text.is_modified());
        text.save().unwrap();
        assert!(!text.is_modified());
        assert!(!text.changed_on_disk().unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        assert_eq!(text.saves(), 1);

        text.add_undo_boundary();
        text.insert(0, "zero\n");
        text.undo();
        assert!(!text.is_modified());
        assert_eq!(text.saves(), 1, "undoing back to the saved text is no save");

        fs::write(&path, "changed elsewhere\n").unwrap();
        assert!(text.changed_on_disk().unwrap());
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_crlf_latin1_file_is_edited_as_utf8_and_saved_as_it_was() {
        let (path, mut text) = opened("latin1-crlf", b"caf\xe9\r\nna\xefve\r\n");
        let format = text.format();
        assert_eq!((format.encoding, format.crlf), (WINDOWS_1252, true));
        assert_eq!(all(&text), "café\nnaïve\n".as_bytes());

        text.insert(text.len(), "über\n");
        text.save().unwrap();
        let saved = b"caf\xe9\r\nna\xefve\r\n\xfcber\r\n";
        assert_eq!(fs::read(&path).unwrap(), saved);

        text.insert(0, "Ж");
        let error = text.save().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(text.is_modified());
        assert_eq!(fs::read(&path).unwrap(), saved);
        fs::remove_file(&path).unwrap();
    }

    /// Above the size files are mapped at, the decoded text goes to a mapped temporary file.
    #[test]
    fn a_large_windows_1251_file_is_decoded_and_saved_back() {
        let line = "Съешь же ещё этих мягких французских булок, да выпей чаю.\r\n";
        let lines = (33 << 20) / 60;
        let contents = WINDOWS_1251.encode(&line.repeat(lines)).0.into_owned();
        let (path, mut text) = opened("mapped-1251", &contents);
        let format = text.format();
        assert_eq!((format.encoding, format.crlf), (WINDOWS_1251, true));
        let first = text.line_range(0).unwrap();
        assert_eq!(text.bytes_in(first), line.trim_end().as_bytes());
        assert_eq!(text.line_count(), lines + 1);

        text.save().unwrap();
        assert!(fs::read(&path).unwrap() == contents);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_binary_file_is_neither_read_nor_saved() {
        let elf = b"\x7fELF\x02\x01\x00\x00\xff";
        let (path, mut text) = opened("binary", elf);
        assert!(text.format().binary);
        assert_eq!(all(&text), b"Binary file of 9 bytes: not shown.\n");
        assert_eq!(text.save().unwrap_err().kind(), io::ErrorKind::Unsupported);
        assert_eq!(fs::read(&path).unwrap(), elf);

        let reread = Text::open_with_encoding(&path, WINDOWS_1252).unwrap();
        assert_eq!(reread.bytes_in(0..4), b"\x7fELF");
        fs::remove_file(&path).unwrap();
    }

    /// The guess sees only the start; a byte its encoding has no character for, later on,
    /// sends the file to windows-1252, which has one for every byte.
    #[test]
    fn a_byte_the_guessed_encoding_lacks_falls_back_to_windows_1252() {
        let line = "Η γρήγορη καφέ αλεπού πηδάει πάνω από τον τεμπέλη σκύλο.\n";
        let mut contents = WINDOWS_1253.encode(&line.repeat(2000)).0.into_owned();
        // 0xFF has no character in windows-1253.
        contents.extend_from_slice(b"\xff\n");
        let (path, mut text) = opened("fallback", &contents);
        assert_eq!(text.format().encoding, WINDOWS_1252);
        text.save().unwrap();
        assert!(fs::read(&path).unwrap() == contents);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn an_encoding_chosen_must_fit_the_bytes() {
        let path = file("chosen", b"caf\xe9\n");
        let error = Text::open_with_encoding(&path, UTF_8).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let text = Text::open_with_encoding(&path, WINDOWS_1251).unwrap();
        assert_eq!(all(&text), "cafй\n".as_bytes());
        fs::remove_file(&path).unwrap();
    }
}
