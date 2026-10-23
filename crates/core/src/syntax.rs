use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_text::Snapshot;

use crate::buffer::Buffer;

/// Names the document of code embedded in a file: `page.html.remacs-embedded.css`.
const EMBEDDED_MARKER: &str = ".remacs-embedded.";

/// What kind of text an offset is in, for commands that type differently in code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxContext {
    Code,
    String,
    Comment,
    /// Markup embedded in code, like JSX elements and their text.
    Markup,
}

/// Tells the context of an offset in a buffer, or `None` when it does not know the buffer.
pub type SyntaxProbe = Rc<dyn Fn(&Buffer, usize) -> Option<SyntaxContext>>;

/// Code of another language in a buffer, like CSS in HTML's `<style>`: its mode and where
/// it is, in order. Servers and formatters are shown it as a document of its own, the
/// file's text blanked around it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Embedded {
    pub mode: &'static str,
    pub ranges: Vec<Range<usize>>,
    /// Whether language servers are shown it; not the script of a Vue component, which
    /// TypeScript alone takes for wrong (`defineProps`, `import './Card.vue'`).
    pub servers: bool,
}

impl Embedded {
    /// Whether `offset` is in one of the ranges or at its end.
    pub fn holds(&self, offset: usize) -> bool {
        self.ranges
            .iter()
            .any(|range| range.start <= offset && offset <= range.end)
    }

    /// Whether `range` is within one of the ranges.
    pub fn holds_range(&self, range: &Range<usize>) -> bool {
        self.ranges
            .iter()
            .any(|code| code.start <= range.start && range.end <= code.end)
    }
}

/// Tells the code of other languages in a buffer, or `None` when it cannot tell now.
pub type EmbeddedProbe = Rc<dyn Fn(&Buffer) -> Option<Vec<Embedded>>>;

/// The document of `file`'s embedded code with `extension`, beside it.
pub fn embedded_document(file: &Path, extension: &str) -> PathBuf {
    let mut path = file.as_os_str().to_owned();
    path.push(EMBEDDED_MARKER);
    path.push(extension);
    PathBuf::from(path)
}

/// The file whose embedded code `path` is the document of; `None` for any other path.
pub fn embedded_file(path: &Path) -> Option<PathBuf> {
    let path = path.to_str()?;
    let end = path.rfind(EMBEDDED_MARKER)?;
    Some(PathBuf::from(&path[..end]))
}

/// `range` of `text` with every byte outside `code`, which is in order, a space, but for
/// line ends: the document of embedded code, at the offsets and lines it has in the file.
pub fn blank_outside(text: &Snapshot, range: Range<usize>, code: &[Range<usize>]) -> String {
    let start = range.start;
    let mut bytes = text.bytes_in(range.clone());
    let mut blank = |gap: Range<usize>| {
        for byte in &mut bytes[gap.start - start..gap.end - start] {
            if !matches!(*byte, b'\n' | b'\r') {
                *byte = b' ';
            }
        }
    };
    let mut kept_to = start;
    for code in code
        .iter()
        .filter(|code| code.end > start && code.start < range.end)
    {
        blank(kept_to..code.start.max(kept_to));
        kept_to = code.end.min(range.end).max(kept_to);
    }
    blank(kept_to..range.end);
    // Whole characters are kept or blanked, since the code's bounds fall between them.
    String::from_utf8(bytes).expect("blanking keeps the text UTF-8")
}

#[cfg(test)]
mod tests {
    use remacs_text::Text;

    use super::*;

    #[test]
    fn embedded_documents_name_their_file() {
        let document = embedded_document(Path::new("/w/page.html"), "css");
        assert_eq!(document, Path::new("/w/page.html.remacs-embedded.css"));
        assert_eq!(
            embedded_file(&document).as_deref(),
            Some(Path::new("/w/page.html"))
        );
        assert_eq!(embedded_file(Path::new("/w/page.html")), None);
    }

    #[test]
    fn blanking_keeps_the_code_the_lines_and_the_offsets() {
        let page = "<p>é</p>\n<style>a{}</style>\n";
        let code = page.find("a{}").unwrap();
        let text = Text::from(page).snapshot();
        let blanked = blank_outside(
            &text,
            0..page.len(),
            &[code..code + 3, page.len()..page.len()],
        );
        assert_eq!(blanked.len(), page.len());
        let expected = format!(
            "{}\n{}a{{}}{}\n",
            " ".repeat(9),
            " ".repeat(7),
            " ".repeat(8)
        );
        assert_eq!(blanked, expected);

        let codes = [1..3, 5..6, 8..20];
        let text = Text::from("0123456789").snapshot();
        assert_eq!(blank_outside(&text, 0..10, &codes), " 12  5  89");
        assert_eq!(blank_outside(&text, 2..7, &codes), "2  5 ");
        assert_eq!(blank_outside(&text, 3..5, &codes), "  ");
    }

    /// A Latin-1 byte used to stay in the text and make the blanked code not UTF-8.
    #[test]
    fn blanking_a_file_that_is_not_utf8_keeps_its_code() {
        let path = std::env::temp_dir().join(format!("remacs-latin1-{}.html", std::process::id()));
        std::fs::write(&path, b"<script>let caf\xe9 = 1;</script>\n").unwrap();
        let text = Text::open(&path).unwrap().snapshot();
        std::fs::remove_file(&path).unwrap();
        let script = 8..text.len() - 10;
        let blanked = blank_outside(&text, 0..text.len(), &[script]);
        assert_eq!(blanked, "        let café = 1;         \n");
    }
}
