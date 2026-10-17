//! Code of other languages in a file, like CSS in HTML's `<style>`, on those languages'
//! servers. Each language's code is a document of its own: the file's text with all else
//! blanked, byte for byte and lines kept, so its offsets are the buffer's.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

use remacs_text::{Edit, Snapshot, Text};

/// Names the document of code embedded in a file: `page.html.remacs-embedded.css`.
const MARKER: &str = ".remacs-embedded.";

/// The document of `file`'s code with `extension`, beside it.
pub(crate) fn path(file: &Path, extension: &str) -> PathBuf {
    let mut path = file.as_os_str().to_owned();
    path.push(MARKER);
    path.push(extension);
    PathBuf::from(path)
}

/// The file whose embedded code `path` names; `None` for any other path.
pub(crate) fn file_of(path: &Path) -> Option<PathBuf> {
    let path = path.to_str()?;
    let end = path.rfind(MARKER)?;
    Some(PathBuf::from(&path[..end]))
}

/// What a server's problems with embedded code are listed under: one checker per server
/// and language, so they do not replace the problems the server has with the file itself.
pub(crate) fn checker(server: &'static str, mode: &'static str) -> &'static str {
    static NAMES: LazyLock<Mutex<HashMap<(&str, &str), &'static str>>> =
        LazyLock::new(Mutex::default);
    let mut names = NAMES.lock().unwrap_or_else(PoisonError::into_inner);
    // Leaked once per server and language, of which there are a few dozen at most.
    names
        .entry((server, mode))
        .or_insert_with(|| Box::leak(format!("{server} ({mode})").into_boxed_str()))
}

/// A buffer's text as the server of one embedded language has it.
pub(crate) struct Mirror {
    pub mode: &'static str,
    /// Where the language's code is, in the buffer's text.
    pub ranges: Vec<Range<usize>>,
    /// The buffer's text it mirrors.
    basis: Snapshot,
    text: Text,
}

impl Mirror {
    pub fn new(mode: &'static str, buffer: Snapshot, ranges: Vec<Range<usize>>) -> Self {
        let text = Text::from(blank(&buffer, 0..buffer.len(), &ranges).as_str());
        Self {
            mode,
            ranges,
            basis: buffer,
            text,
        }
    }

    /// Mirrors `buffer` with the code now at `ranges`; the edit since the last call is
    /// made on the mirror too, unless it moved the code's bounds.
    pub fn update(&mut self, buffer: &Snapshot, ranges: Vec<Range<usize>>) -> Snapshot {
        if buffer.is_same(&self.basis) && ranges == self.ranges {
            return self.text.snapshot();
        }
        match buffer.edit_from(&self.basis) {
            Some(edit) if shifted(&self.ranges, edit) == ranges => {
                self.text.delete(edit.start..edit.old_end);
                let inserted = blank(buffer, edit.start..edit.new_end, &ranges);
                self.text.insert(edit.start, &inserted);
            }
            None if ranges == self.ranges => {}
            _ => self.text = Text::from(blank(buffer, 0..buffer.len(), &ranges).as_str()),
        }
        self.basis = buffer.clone();
        self.ranges = ranges;
        self.text.snapshot()
    }

    pub fn text(&self) -> Snapshot {
        self.text.snapshot()
    }

    /// Whether `range` is in the code.
    pub fn holds(&self, range: &Range<usize>) -> bool {
        self.ranges
            .iter()
            .any(|code| code.start <= range.start && range.end <= code.end)
    }
}

/// `range` of `text` with every byte outside `ranges` a space, but for line ends.
fn blank(text: &Snapshot, range: Range<usize>, ranges: &[Range<usize>]) -> String {
    let start = range.start;
    let mut bytes = text.bytes_in(range);
    for (offset, byte) in (start..).zip(&mut bytes) {
        let kept =
            *byte == b'\n' || *byte == b'\r' || ranges.iter().any(|code| code.contains(&offset));
        if !kept {
            *byte = b' ';
        }
    }
    // Whole characters are kept or blanked, since the code's bounds fall between them.
    String::from_utf8(bytes).expect("blanking keeps the text UTF-8")
}

/// `ranges` moved by `edit` as tree-sitter moves a tree's included ranges.
fn shifted(ranges: &[Range<usize>], edit: Edit) -> Vec<Range<usize>> {
    let moved = |offset: usize| match offset {
        _ if offset >= edit.old_end => offset - edit.old_end + edit.new_end,
        _ if offset > edit.start => edit.start,
        _ => offset,
    };
    ranges
        .iter()
        .map(|range| moved(range.start)..moved(range.end))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contents(snapshot: &Snapshot) -> String {
        String::from_utf8(snapshot.bytes_in(0..snapshot.len())).unwrap()
    }

    #[test]
    fn the_mirror_keeps_the_code_and_the_lines_and_follows_edits() {
        let page = "<p>é</p>\n<style>a{}</style>\n";
        let code = page.find("a{}").unwrap();
        let mut buffer = Text::from(page);
        let style = |range: Range<usize>| Vec::from([range]);
        let mut mirror = Mirror::new("CSS", buffer.snapshot(), style(code..code + 3));
        let blanked = contents(&mirror.update(&buffer.snapshot(), style(code..code + 3)));
        assert_eq!(blanked.len(), page.len());
        assert_eq!(
            blanked,
            format!(
                "{}\n{}a{{}}{}\n",
                " ".repeat(9),
                " ".repeat(7),
                " ".repeat(8)
            )
        );

        buffer.insert(code + 2, "b:0");
        let typed = contents(&mirror.update(&buffer.snapshot(), style(code..code + 6)));
        assert_eq!(typed.trim(), "a{b:0}");
        assert_eq!(typed.find('a'), Some(code));

        buffer.insert(0, "<b>x</b>");
        let moved = style(code + 8..code + 14);
        assert_eq!(
            contents(&mirror.update(&buffer.snapshot(), moved)).trim(),
            "a{b:0}"
        );
    }

    #[test]
    fn embedded_documents_name_their_file() {
        let embedded = path(Path::new("/w/page.html"), "css");
        assert_eq!(embedded, Path::new("/w/page.html.remacs-embedded.css"));
        assert_eq!(
            file_of(&embedded).as_deref(),
            Some(Path::new("/w/page.html"))
        );
        assert_eq!(file_of(Path::new("/w/page.html")), None);
        assert_eq!(checker("css", "CSS"), "css (CSS)");
        assert!(std::ptr::eq(checker("css", "CSS"), checker("css", "CSS")));
    }
}
