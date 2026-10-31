use std::ops::Range;

use remacs_core::{Buffer, CharClass, CommandResult, Editor};

use crate::editing::writable_buffer;

const NOTHING_TO_TRANSPOSE: &str = "Don't have two things to transpose";

/// Swaps the characters around point and moves over them; at the end of a line, the two
/// before point, like Emacs `transpose-chars`.
pub fn transpose_chars(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let text = buffer.text();
    let mut point = buffer.point();
    if buffer.char_after(point).is_none_or(|next| next == '\n') {
        point = text.previous_char_boundary(point);
    }
    if point == 0 || point == text.len() {
        return Err(NOTHING_TO_TRANSPOSE.into());
    }
    let first = text.previous_char_boundary(point)..point;
    let second = point..text.next_char_boundary(point);
    swap(buffer, first, second);
    Ok(())
}

/// Swaps the word before point with the word after it, point after both, like Emacs
/// `transpose-words`.
pub fn transpose_words(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let second_end = skip(buffer, skip(buffer, buffer.point(), false, true), true, true);
    let second_start = skip(buffer, second_end, true, false);
    let first_end = skip(buffer, second_start, false, false);
    let first_start = skip(buffer, first_end, true, false);
    if first_start == first_end || second_start == second_end || first_end > second_start {
        return Err(NOTHING_TO_TRANSPOSE.into());
    }
    swap(buffer, first_start..first_end, second_start..second_end);
    Ok(())
}

/// Swaps the line with the one above it and moves to the next line, like Emacs
/// `transpose-lines`.
pub fn transpose_lines(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let line = buffer.point_line();
    let text = buffer.text();
    let (Some(above), Some(this)) =
        (line.checked_sub(1).and_then(|above| text.line_range(above)), text.line_range(line))
    else {
        return Err(NOTHING_TO_TRANSPOSE.into());
    };
    swap(buffer, above, this);
    let next = buffer.text().line_start(line + 1);
    buffer.set_point(next.unwrap_or(buffer.text().len()));
    Ok(())
}

/// Replaces `first` and `second`, which do not overlap, with each other, point after
/// both.
fn swap(buffer: &mut Buffer, first: Range<usize>, second: Range<usize>) {
    let (first_text, second_text) =
        (buffer.contents(first.clone()), buffer.contents(second.clone()));
    buffer.apply_edits(vec![(second.clone(), first_text), (first, second_text)]);
    buffer.set_point(second.end);
}

/// From `offset`, past the characters that are (`word`) or are not word ones, backward
/// unless `forward`.
fn skip(buffer: &Buffer, mut offset: usize, word: bool, forward: bool) -> usize {
    let text = buffer.text();
    loop {
        let character =
            if forward { buffer.char_after(offset) } else { buffer.char_before(offset) };
        match character {
            Some(character) if (CharClass::of(character) == CharClass::Word) == word => {
                offset = if forward {
                    text.next_char_boundary(offset)
                } else {
                    text.previous_char_boundary(offset)
                };
            }
            _ => return offset,
        }
    }
}
