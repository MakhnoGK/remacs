use std::cmp::Reverse;
use std::ops::Range;

use remacs_core::{Face, Span};
use remacs_text::Snapshot;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Query, QueryCursor, QueryError, TextProvider, Tree};

/// Capture names, most specific first, and the faces they get; other captures
/// (`@variable`, `@property`, `@punctuation`…) keep the default color. Older grammars
/// use nvim-treesitter's former names (`@conditional`, `@text.title`…) and markup ones.
const FACES: &[(&str, Face)] = &[
    ("comment.documentation", Face::Docstring),
    ("comment.doc", Face::Docstring),
    ("comment", Face::Comment),
    ("string", Face::String),
    ("escape", Face::String),
    ("character", Face::String),
    ("text.literal", Face::String),
    ("markup.raw", Face::String),
    ("keyword", Face::Keyword),
    ("conditional", Face::Keyword),
    ("repeat", Face::Keyword),
    ("include", Face::Keyword),
    ("exception", Face::Keyword),
    ("storageclass", Face::Keyword),
    ("variable.builtin", Face::Keyword),
    ("text.title", Face::Keyword),
    ("markup.heading", Face::Keyword),
    ("type", Face::Type),
    ("constructor", Face::Type),
    ("function.macro", Face::Builtin),
    ("function.builtin", Face::Builtin),
    ("attribute", Face::Builtin),
    ("preproc", Face::Builtin),
    ("function", Face::Function),
    ("method", Face::Function),
    ("tag", Face::Function),
    ("constant", Face::Constant),
    ("number", Face::Constant),
    ("float", Face::Constant),
    ("boolean", Face::Constant),
    ("label", Face::Constant),
    ("text.uri", Face::Constant),
    ("text.reference", Face::Constant),
    ("markup.link", Face::Constant),
];

/// Code interpolated into strings (`${…}`, `#{…}`, f-strings' `{…}`) is captured so,
/// and drawn in the default color rather than the string's.
const CODE_CAPTURES: &[&str] = &["embedded", "none"];

/// Strings whose `{…}` placeholders hold code, like Rust's format strings.
const FORMAT_STRING_CAPTURE: &str = "string.format";

/// How a capture draws its node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Paint {
    /// In the face of the node around it.
    Inherit,
    /// In the default color, whatever is around it.
    Plain,
    Face(Face),
}

/// A compiled highlight query and how each of its captures is drawn.
pub(crate) struct Highlighter {
    query: Query,
    paints: Vec<Paint>,
    format_string: Option<u32>,
}

impl Highlighter {
    pub fn new(language: &Language, queries: &[&str]) -> Result<Self, QueryError> {
        let query = Query::new(language, &queries.concat())?;
        let paints = query
            .capture_names()
            .iter()
            .map(|name| paint_for(name))
            .collect();
        let format_string = query.capture_index_for_name(FORMAT_STRING_CAPTURE);
        Ok(Self {
            query,
            paints,
            format_string,
        })
    }

    /// Faces for `range`, sorted and non-overlapping. As in tree-sitter-highlight, a node
    /// captured by several patterns gets the last one's face, and a nested node draws over
    /// the node around it unless its capture has no face.
    pub fn spans(&self, tree: &Tree, text: &Snapshot, range: Range<usize>) -> Vec<Span> {
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());
        let mut matches = cursor.captures(&self.query, tree.root_node(), Chunks(text));
        let mut captures: Vec<(Range<usize>, usize, Paint)> = Vec::new();
        while let Some((matched, index)) = matches.next() {
            let capture = matched.captures()[*index];
            let (node, pattern) = (capture.node.byte_range(), matched.pattern_index);
            if Some(capture.index) == self.format_string {
                let start = node.start;
                captures.extend(
                    placeholders(&text.bytes_in(node.clone()))
                        .map(|held| (start + held.start..start + held.end, pattern, Paint::Plain)),
                );
            }
            captures.push((node, pattern, self.paints[capture.index as usize]));
        }
        captures.sort_by_key(|(range, pattern, _)| (range.start, Reverse(range.end), *pattern));
        captures.dedup_by(|later, earlier| {
            let same_node = later.0 == earlier.0;
            if same_node {
                earlier.2 = later.2;
            }
            same_node
        });
        flatten(
            captures.into_iter().map(|(range, _, face)| (range, face)),
            range,
        )
    }
}

fn face_for(capture_name: &str) -> Option<Face> {
    FACES.iter().find_map(|&(name, face)| {
        let matches = capture_name
            .strip_prefix(name)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'));
        matches.then_some(face)
    })
}

fn paint_for(capture_name: &str) -> Paint {
    if CODE_CAPTURES.contains(&capture_name) {
        return Paint::Plain;
    }
    face_for(capture_name).map_or(Paint::Inherit, Paint::Face)
}

/// The `{…}` placeholders of a format string, `{{` and `}}` being literal braces.
fn placeholders(string: &[u8]) -> impl Iterator<Item = Range<usize>> {
    let mut offset = 0;
    std::iter::from_fn(move || {
        loop {
            let open = offset
                + string
                    .get(offset..)?
                    .iter()
                    .position(|&byte| byte == b'{')?;
            if string.get(open + 1) == Some(&b'{') {
                offset = open + 2;
                continue;
            }
            let close = open + string[open..].iter().position(|&byte| byte == b'}')?;
            offset = close + 1;
            return Some(open..offset);
        }
    })
}

/// Turns nested ranges, outer ones first, into flat spans within `bounds`.
fn flatten(nested: impl Iterator<Item = (Range<usize>, Paint)>, bounds: Range<usize>) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut emit = |range: Range<usize>, face: Option<Face>| {
        let range = range.start.max(bounds.start)..range.end.min(bounds.end);
        let Some(face) = face.filter(|_| !range.is_empty()) else {
            return;
        };
        match spans.last_mut() {
            Some(last) if last.face == face && last.range.end == range.start => {
                last.range.end = range.end;
            }
            _ => spans.push(Span { range, face }),
        }
    };
    // Ends and faces of the ranges around the position, innermost last.
    let mut open: Vec<(usize, Option<Face>)> = Vec::new();
    let mut position = bounds.start;
    for (range, paint) in nested {
        while let Some(&(end, outer_face)) = open.last().filter(|(end, _)| *end <= range.start) {
            emit(position..end, outer_face);
            position = position.max(end);
            open.pop();
        }
        let outer_face = open.last().and_then(|&(_, face)| face);
        emit(position..range.start, outer_face);
        position = position.max(range.start);
        let face = match paint {
            Paint::Inherit => outer_face,
            Paint::Plain => None,
            Paint::Face(face) => Some(face),
        };
        open.push((range.end, face));
    }
    while let Some((end, face)) = open.pop() {
        emit(position..end, face);
        position = position.max(end);
    }
    spans
}

/// Lets query predicates (`#match?`, `#eq?`) read node text straight from the pieces.
pub(crate) struct Chunks<'a>(pub &'a Snapshot);

impl<'a> TextProvider<&'a [u8]> for Chunks<'a> {
    type I = std::vec::IntoIter<&'a [u8]>;

    fn text(&mut self, node: Node) -> Self::I {
        self.0
            .chunks(node.byte_range())
            .collect::<Vec<_>>()
            .into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_names_match_by_dotted_prefix() {
        assert_eq!(face_for("comment.documentation"), Some(Face::Docstring));
        assert_eq!(face_for("function.method"), Some(Face::Function));
        assert_eq!(face_for("type.builtin"), Some(Face::Type));
        assert_eq!(face_for("typed"), None);
        assert_eq!(face_for("variable"), None);
    }

    #[test]
    fn inner_ranges_draw_over_outer_ones() {
        let nested = [
            (0..10, Paint::Face(Face::String)),
            (2..4, Paint::Face(Face::Constant)),
            (5..6, Paint::Inherit),
            (7..9, Paint::Plain),
            (7..8, Paint::Inherit),
            (12..14, Paint::Face(Face::Keyword)),
        ];
        let spans: Vec<_> = flatten(nested.into_iter(), 1..13)
            .into_iter()
            .map(|span| (span.range, span.face))
            .collect();
        assert_eq!(
            spans,
            [
                (1..2, Face::String),
                (2..4, Face::Constant),
                (4..7, Face::String),
                (9..10, Face::String),
                (12..13, Face::Keyword),
            ]
        );
    }

    #[test]
    fn placeholders_skip_escaped_braces() {
        let found: Vec<_> = placeholders(br#""{{a}} {b} {0:>8} {}" {"#).collect();
        assert_eq!(found, [7..10, 11..17, 18..20]);
    }
}
