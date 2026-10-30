//! Highlighting where no grammar parses, in a mode without one or a file too big to parse:
//! line by line, as each line reads alone, from the mode's comment syntax and quotes.

use std::ops::Range;

use remacs_core::{Buffer, Face, Mode, Span};

use crate::highlighter::face_for;

/// What a buffer's mode says about its code.
pub(crate) struct Fallback {
    pub mode: &'static str,
    comment: Option<(&'static str, &'static str)>,
    quotes: Vec<u8>,
}

impl Fallback {
    /// For `mode`, if it has a comment syntax or quotes to go by.
    pub fn of(mode: &Mode) -> Option<Self> {
        let comment = mode
            .comment
            .map(|comment| (comment.start.trim_end(), comment.end.trim_start()))
            .filter(|(start, _)| !start.is_empty());
        let quotes: Vec<u8> = (mode.pairs.iter())
            .filter(|(open, close)| open == close && open.is_ascii())
            .map(|&(quote, _)| quote as u8)
            .collect();
        (comment.is_some() || !quotes.is_empty()).then_some(Self {
            mode: mode.name,
            comment,
            quotes,
        })
    }
}

/// Comments, strings and numbers on the lines of `range`, each read only within it.
// shortcut: a string or comment begun left of `range` on its line goes unseen; a file
// this big is often one line of megabytes, read on every redraw otherwise.
pub(crate) fn highlight(buffer: &Buffer, range: Range<usize>, level: u8) -> Vec<Span> {
    let Some(fallback) = buffer.local::<Fallback>() else {
        return Vec::new();
    };
    let text = buffer.text();
    let lines = text.line_of(range.start)..=text.line_of(range.end.min(text.len()));
    let shown = |face: &Face| {
        let capture = match face {
            Face::Comment => "comment",
            Face::String => "string",
            _ => "number",
        };
        face_for(capture).is_some_and(|(_, needed)| needed <= level)
    };
    let mut spans = Vec::new();
    for line in lines {
        let Some(bounds) = text.line_range(line) else {
            continue;
        };
        let start = bounds.start.max(range.start);
        let bytes = text.bytes_in(start..bounds.end.min(range.end).max(start));
        spans.extend(
            scan(fallback, &bytes)
                .into_iter()
                .filter(|(_, face)| shown(face))
                .map(|(span, face)| Span { range: start + span.start..start + span.end, face }),
        );
    }
    spans
}

fn scan(fallback: &Fallback, line: &[u8]) -> Vec<(Range<usize>, Face)> {
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut found = Vec::new();
    let mut at = 0;
    while at < line.len() {
        let rest = &line[at..];
        if let Some((start, end)) = fallback.comment
            && rest.starts_with(start.as_bytes())
        {
            let close = (!end.is_empty())
                .then(|| find(&rest[start.len()..], end.as_bytes()))
                .flatten()
                .map_or(line.len(), |close| at + start.len() + close + end.len());
            found.push((at..close, Face::Comment));
            at = close;
        } else if fallback.quotes.contains(&rest[0]) {
            let quote = rest[0];
            let mut close = at + 1;
            while close < line.len() && line[close] != quote {
                close += if line[close] == b'\\' { 2 } else { 1 };
            }
            let close = (close + 1).min(line.len());
            found.push((at..close, Face::String));
            at = close;
        } else if rest[0].is_ascii_digit() && (at == 0 || !is_word(line[at - 1])) {
            let end = at + rest.iter().take_while(|&&byte| is_word(byte) || byte == b'.').count();
            found.push((at..end, Face::Constant));
            at = end;
        } else {
            at += 1;
        }
    }
    found
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_reads_its_comment_strings_and_numbers() {
        let fallback =
            Fallback { mode: "Code", comment: Some(("//", "")), quotes: vec![b'"', b'\''] };
        let line = br#"x = "a\"b // no" + 'c' + 0x1f + v2; // done "x""#;
        let faces: Vec<_> = scan(&fallback, line)
            .into_iter()
            .map(|(range, face)| (std::str::from_utf8(&line[range]).unwrap(), face))
            .collect();
        assert_eq!(
            faces,
            [
                (r#""a\"b // no""#, Face::String),
                ("'c'", Face::String),
                ("0x1f", Face::Constant),
                (r#"// done "x""#, Face::Comment),
            ]
        );
        let css = Fallback { mode: "Style", comment: Some(("/*", "*/")), quotes: Vec::new() };
        let faces = scan(&css, b"a /* b */ 2");
        assert_eq!(faces, [(2..9, Face::Comment), (10..11, Face::Constant)]);
    }
}
