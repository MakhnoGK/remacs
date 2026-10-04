use std::ops::Range;

use remacs_core::{Buffer, Editor, SyntaxContext};

/// An opener pairs only before these, the end of a line or a closer, so typing in front
/// of a word does not leave a stray closer.
const PAIR_BEFORE: &str = ";:.,=> \t\r\n";
/// Bounds the balance check on long lines.
const MAX_SCAN: usize = 64 << 10;

/// The current buffer's bracket and quote pairs; none in Fundamental mode.
pub fn mode_pairs(editor: &Editor) -> &'static [(char, char)] {
    editor.buffer_mode().map_or(&[], |mode| mode.pairs)
}

/// Types `character` as part of a pair: an opener wraps the region or inserts its closer
/// too, a closer steps over the same closer when the line's pairs are balanced. Quotes
/// pair only in code and step over only out of a string. Returns whether it did;
/// otherwise the character is typed as usual.
pub fn type_paired(
    buffer: &mut Buffer,
    pairs: &[(char, char)],
    context: SyntaxContext,
    character: char,
) -> bool {
    if pairs.contains(&(character, character)) {
        return type_quote(buffer, pairs, context, character);
    }
    if let Some(&(opener, closer)) = pairs.iter().find(|(opener, _)| *opener == character) {
        if let Some(region) = buffer.region() {
            wrap(buffer, region, opener, closer);
            return true;
        }
        let pairs_here = pairs_before_next(buffer, pairs);
        if pairs_here {
            let point = buffer.point();
            buffer.insert(&format!("{opener}{closer}"));
            buffer.set_point(point + opener.len_utf8());
        }
        return pairs_here;
    }
    let Some(&(opener, closer)) = pairs.iter().find(|(_, closer)| *closer == character) else {
        return false;
    };
    let point = buffer.point();
    let steps_over =
        buffer.char_after(point) == Some(closer) && is_balanced_on_line(buffer, opener, closer);
    if steps_over {
        buffer.set_point(point + closer.len_utf8());
    }
    steps_over
}

fn type_quote(
    buffer: &mut Buffer,
    pairs: &[(char, char)],
    context: SyntaxContext,
    quote: char,
) -> bool {
    if let Some(region) = buffer.region() {
        wrap(buffer, region, quote, quote);
        return true;
    }
    let point = buffer.point();
    let before = buffer.char_before(point);
    // An empty pair just typed may not be parsed yet, so it is recognised by its text.
    let closes_string = context == SyntaxContext::String || before == Some(quote);
    if buffer.char_after(point) == Some(quote) && closes_string {
        buffer.set_point(point + quote.len_utf8());
        return true;
    }
    let after_word = before.is_some_and(|before| before.is_alphanumeric() || before == '_');
    let pairs_here =
        context == SyntaxContext::Code && !after_word && pairs_before_next(buffer, pairs);
    if pairs_here {
        buffer.insert(&format!("{quote}{quote}"));
        buffer.set_point(point + quote.len_utf8());
    }
    pairs_here
}

/// Whether the character after point lets an opener pair: a blank, the end of the line,
/// a closing bracket or one of `PAIR_BEFORE`, not a word or a quote.
fn pairs_before_next(buffer: &Buffer, pairs: &[(char, char)]) -> bool {
    buffer.char_after(buffer.point()).is_none_or(|next| {
        PAIR_BEFORE.contains(next)
            || pairs
                .iter()
                .any(|&(opener, closer)| closer == next && opener != closer)
    })
}

/// The empty pair point stands in, as in `(|)`, for DEL to delete whole.
pub fn empty_pair_around(buffer: &Buffer, pairs: &[(char, char)]) -> Option<Range<usize>> {
    let point = buffer.point();
    let before = buffer.char_before(point)?;
    let after = buffer.char_after(point)?;
    pairs
        .contains(&(before, after))
        .then(|| point - before.len_utf8()..point + after.len_utf8())
}

/// Encloses the region in the pair and keeps it marked, so it can be wrapped again.
fn wrap(buffer: &mut Buffer, region: Range<usize>, opener: char, closer: char) {
    let point_at_end = buffer.point() == region.end;
    buffer.set_point(region.end);
    buffer.insert(&closer.to_string());
    buffer.set_point(region.start);
    buffer.insert(&opener.to_string());
    let inner = region.start + opener.len_utf8()..region.end + opener.len_utf8();
    let (mark, point) = if point_at_end {
        (inner.start, inner.end)
    } else {
        (inner.end, inner.start)
    };
    buffer.set_mark(mark);
    buffer.set_point(point);
    buffer.activate_mark();
}

/// Whether the line at point closes as many `opener`s as it opens, so another closer
/// would be one too many.
fn is_balanced_on_line(buffer: &Buffer, opener: char, closer: char) -> bool {
    let text = buffer.text();
    let point = buffer.point();
    let line = text.line_range(buffer.point_line()).unwrap_or(point..point);
    let start = line.start.max(point.saturating_sub(MAX_SCAN));
    let end = line.end.min(point + MAX_SCAN);
    let line = String::from_utf8_lossy(&text.bytes_in(start..end)).into_owned();
    let opened = line
        .chars()
        .filter(|&character| character == opener)
        .count();
    let closed = line
        .chars()
        .filter(|&character| character == closer)
        .count();
    opened <= closed
}
