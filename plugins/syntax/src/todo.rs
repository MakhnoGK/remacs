//! TODO, FIXME and the like marked in comments, as Emacs `hl-todo` does.

use std::ops::Range;

use remacs_core::{Buffer, Face, Span};

/// `spans` with each of `keywords` that stands as a whole word in a comment marked as a
/// keyword.
pub(crate) fn mark(buffer: &Buffer, spans: Vec<Span>, keywords: &[String]) -> Vec<Span> {
    if keywords.is_empty() {
        return spans;
    }
    let mut marked = Vec::with_capacity(spans.len());
    for span in spans {
        if !matches!(span.face, Face::Comment | Face::Docstring) {
            marked.push(span);
            continue;
        }
        let text = buffer.text().bytes_in(span.range.clone());
        let offset = span.range.start;
        let mut start = 0;
        for word in words(&text, keywords) {
            push(&mut marked, offset + start..offset + word.start, span.face);
            push(&mut marked, offset + word.start..offset + word.end, Face::Keyword);
            start = word.end;
        }
        push(&mut marked, offset + start..span.range.end, span.face);
    }
    marked
}

fn push(spans: &mut Vec<Span>, range: Range<usize>, face: Face) {
    if !range.is_empty() {
        spans.push(Span { range, face });
    }
}

/// Where `keywords` stand in `text` as whole words.
fn words(text: &[u8], keywords: &[String]) -> Vec<Range<usize>> {
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut found = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let starts_word = at == 0 || !is_word(text[at - 1]);
        let keyword = keywords.iter().map(String::as_bytes).find(|keyword| {
            starts_word
                && text[at..].starts_with(keyword)
                && text.get(at + keyword.len()).is_none_or(|&next| !is_word(next))
        });
        match keyword {
            Some(keyword) => {
                found.push(at..at + keyword.len());
                at += keyword.len();
            }
            None => at += 1,
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_are_found_as_whole_words_only() {
        let keywords = ["TODO".to_owned(), "FIXME".to_owned()];
        let found = words(b"// TODO: x FIXME(me) TODOS _TODO TODO", &keywords);
        assert_eq!(found, [3..7, 11..16, 33..37]);
    }
}
