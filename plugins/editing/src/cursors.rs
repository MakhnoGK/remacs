//! More cursors that commands run at too, like Emacs multiple-cursors and iedit: added at
//! the next match of the region, at every match, on each line of the region, or at each
//! occurrence of the symbol at point.

use std::ops::Range;

use memchr::memmem;
use remacs_core::{Buffer, CharClass, CommandResult, Editor, StatusItem, StatusSegment, Tone};

use crate::EditingSettings;

pub const MARK_NEXT: &str = "mc/mark-next-like-this";
pub const MARK_PREVIOUS: &str = "mc/mark-previous-like-this";
pub const SKIP_TO_NEXT: &str = "mc/skip-to-next-like-this";
pub const MARK_ALL: &str = "mc/mark-all-like-this";
pub const EDIT_LINES: &str = "mc/edit-lines";
pub const IEDIT_MODE: &str = "iedit-mode";
pub const REMOVE_CURSORS: &str = "mc/remove-cursors";

/// Bytes read at a time when searching, so a file of gigabytes is never copied whole.
const SEARCHED_BLOCK: usize = 64 * 1024;

pub fn has_cursors(editor: &Editor) -> bool {
    !editor.buffer().cursors().is_empty()
}

/// Adds a cursor at the next match of the region after the last cursor, or without a
/// region, on the line under it, like Emacs `mc/mark-next-like-this`.
pub fn mark_next_like_this(editor: &mut Editor) -> CommandResult {
    mark_like_this(editor, true)
}

pub fn mark_previous_like_this(editor: &mut Editor) -> CommandResult {
    mark_like_this(editor, false)
}

fn mark_like_this(editor: &mut Editor, forward: bool) -> CommandResult {
    let buffer = editor.buffer();
    let Some(region) = buffer.region() else {
        return add_on_next_line(editor, forward);
    };
    let needle = buffer.contents(region.clone());
    let (first, last) = span_of_cursors(buffer);
    let found = if forward {
        find_forward(buffer, last, needle.as_bytes())
    } else {
        find_backward(buffer, first, needle.as_bytes())
    }
    .ok_or_else(|| format!("No more matches for “{needle}”"))?;
    let (point, mark) = like_region(buffer, found..found + needle.len());
    add(editor, vec![(point, Some(mark))])
}

/// Moves the last cursor added after point to the next match of the region, or the
/// region itself without one, like Emacs `mc/skip-to-next-like-this`.
pub fn skip_to_next_like_this(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    let region = buffer.region().ok_or("Mark a region to match first")?;
    let needle = buffer.contents(region.clone());
    let from = match buffer.cursors().last().filter(|last| last.point > region.end) {
        Some(_) => {
            let cursors = buffer.cursors().len();
            let last = buffer.remove_cursor(cursors - 1);
            last.region().map_or(last.point, |region| region.end)
        }
        None => region.end,
    };
    let found = find_forward(buffer, from, needle.as_bytes())
        .ok_or_else(|| format!("No more matches for “{needle}”"))?;
    let (point, mark) = like_region(buffer, found..found + needle.len());
    if buffer.cursors().is_empty() && from == region.end {
        buffer.set_mark(mark);
        buffer.set_point(point);
        buffer.activate_mark();
        return Ok(());
    }
    add(editor, vec![(point, Some(mark))])
}

/// Adds a cursor at every match of the region, like Emacs `mc/mark-all-like-this`.
pub fn mark_all_like_this(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer();
    let region = buffer.region().ok_or("Mark a region to match first")?;
    let needle = buffer.contents(region.clone());
    let places: Vec<_> = find_all(buffer, needle.as_bytes(), |_| true)
        .into_iter()
        .filter(|&found| found != region.start)
        .map(|found| {
            let (point, mark) = like_region(buffer, found..found + needle.len());
            (point, Some(mark))
        })
        .collect();
    add(editor, places)?;
    report_count(editor);
    Ok(())
}

/// Adds a cursor on each line of the region at point's column, or at the end of a shorter
/// line, and deactivates the region, like Emacs `mc/edit-lines`.
pub fn edit_lines(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    let region = buffer.region().ok_or("Mark a region of lines first")?;
    let text = buffer.text();
    let (first, last, column) =
        (text.line_of(region.start), text.line_of(region.end), buffer.point_column());
    let point_line = buffer.point_line();
    let places: Vec<_> = (first..=last)
        .filter(|&line| line != point_line)
        .map(|line| (text.offset_at_char_column(line, column), None))
        .collect();
    add(editor, places)?;
    editor.buffer_mut().deactivate_mark();
    report_count(editor);
    Ok(())
}

/// Puts a cursor at every occurrence of the symbol at point, or of the region, as far
/// into it as point is; run again, removes them, like Emacs `iedit-mode`.
pub fn iedit_mode(editor: &mut Editor) -> CommandResult {
    if has_cursors(editor) {
        editor.buffer_mut().clear_cursors();
        editor.set_message("iedit: done");
        return Ok(());
    }
    let buffer = editor.buffer();
    let (occurrence, whole_symbol) = match buffer.region() {
        Some(region) => (region, false),
        None => (symbol_at(buffer, buffer.point()).ok_or("No symbol at point")?, true),
    };
    let needle = buffer.contents(occurrence.clone());
    let into = buffer.point() - occurrence.start;
    let matches = find_all(buffer, needle.as_bytes(), |found| {
        !whole_symbol || symbol_at(buffer, found) == Some(found..found + needle.len())
    });
    let places = matches
        .into_iter()
        .filter(|&found| found != occurrence.start)
        .map(|found| (found + into, None))
        .collect();
    add(editor, places)?;
    editor.buffer_mut().deactivate_mark();
    report_count(editor);
    Ok(())
}

/// Deactivates the regions, or with none active, removes the cursors: `keyboard-quit`
/// itself, so that keymaps over the global one, like a popup's, still take C-g first.
pub fn keyboard_quit(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    let any_region = buffer.region().is_some()
        || buffer.cursors().iter().any(|cursor| cursor.region().is_some());
    if any_region {
        buffer.deactivate_mark();
        buffer.deactivate_cursor_marks();
    } else {
        buffer.clear_cursors();
    }
    editor.set_message("Quit");
    Ok(())
}

pub fn remove_cursors(editor: &mut Editor) -> CommandResult {
    editor.buffer_mut().clear_cursors();
    Ok(())
}

pub fn status(editor: &Editor) -> Option<StatusSegment> {
    let count = editor.window_buffer().cursors().len();
    (count > 0).then(|| StatusSegment {
        items: vec![StatusItem::toned(format!("{} cursors", count + 1), Tone::Accent)],
        command: Some(REMOVE_CURSORS),
        active: false,
        code: false,
    })
}

fn report_count(editor: &mut Editor) {
    let count = editor.buffer().cursors().len() + 1;
    editor.set_message(format!("{count} cursors"));
}

/// Point and mark over `matched`, point on the same side of it as on the region.
fn like_region(buffer: &Buffer, matched: Range<usize>) -> (usize, usize) {
    let point_at_end = buffer.region().is_some_and(|region| buffer.point() == region.end);
    if point_at_end { (matched.end, matched.start) } else { (matched.start, matched.end) }
}

fn add_on_next_line(editor: &mut Editor, down: bool) -> CommandResult {
    let buffer = editor.buffer();
    let (first, last) = span_of_cursors(buffer);
    let text = buffer.text();
    let line = if down {
        Some(text.line_of(last) + 1).filter(|&line| line < text.line_count())
    } else {
        text.line_of(first).checked_sub(1)
    }
    .ok_or(if down { "End of buffer" } else { "Beginning of buffer" })?;
    let offset = text.offset_at_char_column(line, buffer.point_column());
    add(editor, vec![(offset, None)])
}

/// Adds cursors at `places`, each with its mark if any, unless there would be more than
/// `max-cursors`: each edit moves every cursor after it, so a key costs their square.
fn add(editor: &mut Editor, places: Vec<(usize, Option<usize>)>) -> CommandResult {
    let max = editor.plugin_state::<EditingSettings>().max_cursors;
    let buffer = editor.buffer();
    // Counted as they will be once those on point or on a cursor have merged.
    let mut new: Vec<usize> = places
        .iter()
        .map(|&(point, _)| point)
        .filter(|&point| {
            point != buffer.point()
                && buffer.cursors().binary_search_by_key(&point, |cursor| cursor.point).is_err()
        })
        .collect();
    new.sort_unstable();
    new.dedup();
    let total = buffer.cursors().len() + new.len() + 1;
    if total > max {
        return Err(format!("{total} cursors would be over max-cursors, {max}").into());
    }
    let kill_ring = editor.kill_ring.clone();
    editor.buffer_mut().add_cursors(places, &kill_ring);
    Ok(())
}

/// Where the first cursor, or point, starts and the last one ends, regions included.
fn span_of_cursors(buffer: &Buffer) -> (usize, usize) {
    let point = buffer.region().unwrap_or(buffer.point()..buffer.point());
    buffer.cursors().iter().fold((point.start, point.end), |(first, last), cursor| {
        let at = cursor.region().unwrap_or(cursor.point..cursor.point);
        (first.min(at.start), last.max(at.end))
    })
}

/// The identifier around `offset`, if it is in or right after one.
fn symbol_at(buffer: &Buffer, offset: usize) -> Option<Range<usize>> {
    let is_symbol = |character: Option<char>| {
        character.is_some_and(|character| CharClass::of(character) == CharClass::Word)
    };
    let text = buffer.text();
    let (mut start, mut end) = (offset, offset);
    while is_symbol(buffer.char_before(start)) {
        start = text.previous_char_boundary(start);
    }
    while is_symbol(buffer.char_after(end)) {
        end = text.next_char_boundary(end);
    }
    (start < end).then_some(start..end)
}

/// The start of each match of `needle` that `keep` accepts, matches not overlapping; each
/// block of the text is read once. Overlapping occurrences are skipped even when `keep`
/// refuses the first, which loses nothing to whole symbols: two that overlap are both
/// inside a longer one.
fn find_all(buffer: &Buffer, needle: &[u8], keep: impl Fn(usize) -> bool) -> Vec<usize> {
    let text = buffer.text();
    let finder = memmem::Finder::new(needle);
    let block = SEARCHED_BLOCK.max(2 * needle.len());
    let (mut matches, mut start, mut next_allowed) = (Vec::new(), 0, 0);
    while !needle.is_empty() && start + needle.len() <= text.len() {
        let end = (start + block).min(text.len());
        for found in finder.find_iter(&text.bytes_in(start..end)) {
            let found = start + found;
            if found >= next_allowed && keep(found) {
                matches.push(found);
                next_allowed = found + needle.len();
            }
        }
        if end == text.len() {
            break;
        }
        // The next block starts early enough for a match across the two.
        start = end + 1 - needle.len();
    }
    matches
}

/// The first match of `needle` that starts at or after `from`.
pub(crate) fn find_forward(buffer: &Buffer, from: usize, needle: &[u8]) -> Option<usize> {
    let text = buffer.text();
    let block = SEARCHED_BLOCK.max(2 * needle.len());
    let mut start = from;
    while start + needle.len() <= text.len() && !needle.is_empty() {
        let end = (start + block).min(text.len());
        if let Some(at) = memmem::find(&text.bytes_in(start..end), needle) {
            return Some(start + at);
        }
        // The next block starts early enough for a match across the two.
        start = end + 1 - needle.len();
        if end == text.len() {
            return None;
        }
    }
    None
}

/// The last match of `needle` that ends at or before `before`.
fn find_backward(buffer: &Buffer, before: usize, needle: &[u8]) -> Option<usize> {
    let text = buffer.text();
    let block = SEARCHED_BLOCK.max(2 * needle.len());
    let mut end = before;
    while end >= needle.len() && !needle.is_empty() {
        let start = end.saturating_sub(block);
        if let Some(at) = memmem::rfind(&text.bytes_in(start..end), needle) {
            return Some(start + at);
        }
        if start == 0 {
            return None;
        }
        end = start + needle.len() - 1;
    }
    None
}
