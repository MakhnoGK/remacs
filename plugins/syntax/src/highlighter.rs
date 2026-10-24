use std::cmp::Reverse;
use std::ops::Range;

use remacs_core::{Face, Span};
use remacs_text::Snapshot;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Query, QueryCursor, QueryError, TextProvider, Tree};

/// The capture names of remacs's queries (`plugins/languages/queries`, ADR 0081), most
/// specific first, the faces they get and the highlight level from which they get them;
/// a name matches its dotted extensions too (`function.call` is a `function`). Others keep
/// the default color.
const FACES: &[(&str, Face, u8)] = &[
    ("comment.doc", Face::Docstring, 1),
    ("comment", Face::Comment, 1),
    ("string", Face::String, 2),
    ("markup.raw", Face::String, 2),
    ("keyword", Face::Keyword, 2),
    ("variable.builtin", Face::Keyword, 2),
    ("markup.heading", Face::Keyword, 1),
    ("type", Face::Type, 2),
    ("function.macro", Face::Builtin, 3),
    ("function.builtin", Face::Builtin, 3),
    ("attribute", Face::Builtin, 3),
    ("preproc", Face::Builtin, 2),
    ("function.call", Face::Function, 4),
    // Definitions: calls are `function.call`.
    ("function", Face::Function, 1),
    ("tag", Face::Function, 2),
    // Keys of data files.
    ("property", Face::Function, 2),
    ("diff.plus", Face::Added, 1),
    ("diff.minus", Face::Removed, 1),
    ("constant", Face::Constant, 3),
    ("number", Face::Constant, 3),
    ("boolean", Face::Constant, 3),
    ("label", Face::Constant, 3),
    ("markup.link", Face::Constant, 3),
];

/// Code interpolated into strings (`${…}`, `#{…}`, f-strings' `{…}`) is captured so,
/// and drawn in the default color rather than the string's.
const CODE_CAPTURE: &str = "embedded";

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
    /// None for a capture that draws nothing; else its paint and the level it needs.
    paints: Vec<Option<(Paint, u8)>>,
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
    /// the node around it unless its capture has no face. Captures above `level` are kept
    /// but draw no face of their own, so that `FOO` read as a constant does not fall back
    /// to the type its capitalized-name pattern before says.
    pub fn spans(&self, tree: &Tree, text: &Snapshot, range: Range<usize>, level: u8) -> Vec<Span> {
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
            // A `_`-named capture is only read by predicates: it must not take the node's face
            // from a pattern before, like `println`'s from its macro call's.
            if let Some((paint, needed)) = self.paints[capture.index as usize] {
                let paint = if needed > level {
                    Paint::Inherit
                } else {
                    paint
                };
                captures.push((node, pattern, paint));
            }
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

pub(crate) fn face_for(capture_name: &str) -> Option<(Face, u8)> {
    FACES.iter().find_map(|&(name, face, level)| {
        let matches = capture_name
            .strip_prefix(name)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'));
        matches.then_some((face, level))
    })
}

/// Whether a highlight query's capture is one of the scheme's, or a `_`-named one that
/// only predicates read; anything else would be a typo drawn in the default color.
pub fn known_capture(capture_name: &str) -> bool {
    capture_name.starts_with('_')
        || [CODE_CAPTURE, FORMAT_STRING_CAPTURE].contains(&capture_name)
        || face_for(capture_name).is_some()
}

fn paint_for(capture_name: &str) -> Option<(Paint, u8)> {
    if capture_name.starts_with('_') {
        return None;
    }
    if capture_name == CODE_CAPTURE {
        return Some((Paint::Plain, 1));
    }
    Some(
        face_for(capture_name).map_or((Paint::Inherit, 1), |(face, level)| {
            (Paint::Face(face), level)
        }),
    )
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
        assert_eq!(face_for("comment.doc"), Some((Face::Docstring, 1)));
        assert_eq!(face_for("function.call.method"), Some((Face::Function, 4)));
        assert_eq!(face_for("function"), Some((Face::Function, 1)));
        assert_eq!(face_for("type.builtin"), Some((Face::Type, 2)));
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
