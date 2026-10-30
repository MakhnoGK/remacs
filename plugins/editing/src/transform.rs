use std::ops::Range;

use remacs_core::{Buffer, CommandResult, Editor, Key, describe_sequence};

use crate::cursors::find_forward;
use crate::editing::writable_buffer;
use crate::lines::lines_of;
use crate::motion::{cycle_step, forward_word_end};

pub const ZAP_TO_CHAR: &str = "zap-to-char";
pub const ZAP_UP_TO_CHAR: &str = "zap-up-to-char";
pub const CYCLE_SPACING: &str = "cycle-spacing";

pub fn upcase_dwim(editor: &mut Editor) -> CommandResult {
    change_case(editor, str::to_uppercase)
}

pub fn downcase_dwim(editor: &mut Editor) -> CommandResult {
    change_case(editor, str::to_lowercase)
}

pub fn capitalize_dwim(editor: &mut Editor) -> CommandResult {
    change_case(editor, capitalize)
}

/// Changes the case of the region, or of the text up to the end of the next word, point
/// going there, like Emacs `upcase-dwim`.
fn change_case(editor: &mut Editor, change: fn(&str) -> String) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let region = buffer.region();
    let range =
        region.clone().unwrap_or_else(|| buffer.point()..forward_word_end(buffer, buffer.point()));
    let changed = change(&buffer.contents(range.clone()));
    let end = range.start + changed.len();
    buffer.apply_edits(vec![(range, changed)]);
    if region.is_none() {
        buffer.set_point(end);
    }
    Ok(())
}

/// Each word's first letter upper case, the others lower, like Emacs `capitalize-region`.
fn capitalize(text: &str) -> String {
    let mut in_word = false;
    text.chars()
        .flat_map(|character| {
            let first = !in_word;
            in_word = character.is_alphanumeric();
            if first {
                character.to_uppercase().collect::<Vec<_>>()
            } else {
                character.to_lowercase().collect()
            }
        })
        .collect()
}

/// What `zap-to-char` waits for: the next key typed is the character.
#[derive(Default)]
struct Zapping(Option<&'static str>);

/// Reads a character, then kills up to and with its next occurrence, like Emacs.
pub fn zap_to_char(editor: &mut Editor) -> CommandResult {
    start_zapping(editor, ZAP_TO_CHAR)
}

/// Like `zap-to-char`, but leaves the character.
pub fn zap_up_to_char(editor: &mut Editor) -> CommandResult {
    start_zapping(editor, ZAP_UP_TO_CHAR)
}

fn start_zapping(editor: &mut Editor, command: &'static str) -> CommandResult {
    // Key sinks take no keys while a prompt is open, so the character would come later.
    if editor.minibuffer().is_some() {
        return Err("Not in the minibuffer".into());
    }
    writable_buffer(editor)?;
    editor.plugin_state::<Zapping>().0 = Some(command);
    let prompt = if command == ZAP_TO_CHAR { "Zap to char: " } else { "Zap up to char: " };
    editor.set_message(prompt);
    Ok(())
}

/// The key sink taking the character `zap-to-char` waits for.
pub(crate) fn take_zap_key(editor: &mut Editor, key: Key) -> bool {
    let Some(command) = editor.plugin_state::<Zapping>().0.take() else {
        return false;
    };
    let Some(character) = key.printable_char() else {
        let typed = describe_sequence(&[key]);
        editor.set_message(if typed == "C-g" {
            "Quit".into()
        } else {
            format!("{typed} is not a character")
        });
        return true;
    };
    if let Err(error) = zap(editor, command, character) {
        editor.set_error(error.0);
    }
    true
}

fn zap(editor: &mut Editor, command: &str, character: char) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let point = buffer.point();
    let needle = character.encode_utf8(&mut [0; 4]).as_bytes().to_owned();
    let found = find_forward(buffer, point, &needle)
        .ok_or_else(|| format!("Search failed: “{character}”"))?;
    let end = if command == ZAP_TO_CHAR { found + needle.len() } else { found };
    buffer.add_undo_boundary();
    editor.kill(point..end);
    Ok(())
}

/// Joins the line to the one above, one space between unless after an opening bracket,
/// before a closing one or at the start of the line; with a region, every line of it
/// onto the first, like Emacs `delete-indentation`.
pub fn join_line(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let lines = match buffer.region() {
        Some(region) => {
            let text = buffer.text();
            text.line_of(region.start) + 1..text.line_of(region.end) + 1
        }
        None => buffer.point_line()..buffer.point_line() + 1,
    };
    if lines.start == 0 {
        return Err("Beginning of buffer".into());
    }
    let mut join = buffer.point();
    for line in lines.rev() {
        let Some(start) = buffer.text().line_start(line) else {
            continue;
        };
        let newline = start - 1;
        let before = skip_blanks(buffer, newline, false);
        let after = skip_blanks(buffer, start, true);
        let tight = before == buffer.text().line_start(line - 1).unwrap_or(0)
            || buffer.char_before(before).is_some_and(|c| "([{".contains(c))
            || buffer.char_after(after).is_none_or(|c| ")]}\n".contains(c));
        let space = if tight { "" } else { " " };
        buffer.apply_edits(vec![(before..after, space.into())]);
        join = before;
    }
    buffer.set_point(join);
    Ok(())
}

/// The whitespace around point, then none, then what was there, like Emacs
/// `cycle-spacing`.
pub fn cycle_spacing(editor: &mut Editor) -> CommandResult {
    #[derive(Default)]
    struct Original {
        start: usize,
        text: String,
        point: usize,
    }
    writable_buffer(editor)?;
    let step = cycle_step(editor, CYCLE_SPACING, 3);
    if step == 0 {
        let buffer = editor.buffer();
        let point = buffer.point();
        let blanks = skip_blanks(buffer, point, false)..skip_blanks(buffer, point, true);
        *editor.plugin_state::<Original>() =
            Original { start: blanks.start, text: buffer.contents(blanks.clone()), point };
        replace(editor.buffer_mut(), blanks, " ");
        return Ok(());
    }
    let Original { start, text, point } = std::mem::take(editor.plugin_state::<Original>());
    let buffer = editor.buffer_mut();
    if step == 1 {
        replace(buffer, start..start + 1, "");
    } else {
        replace(buffer, start..start, &text);
        buffer.set_point(point);
    }
    *editor.plugin_state::<Original>() = Original { start, text, point };
    Ok(())
}

fn replace(buffer: &mut Buffer, range: Range<usize>, text: &str) {
    let end = range.start + text.len();
    buffer.apply_edits(vec![(range, text.into())]);
    buffer.set_point(end);
}

/// From `offset`, past the spaces and tabs forward, or backward unless `forward`.
fn skip_blanks(buffer: &Buffer, mut offset: usize, forward: bool) -> usize {
    let is_blank = |character: Option<char>| matches!(character, Some(' ' | '\t'));
    if forward {
        while is_blank(buffer.char_after(offset)) {
            offset += 1;
        }
    } else {
        while is_blank(buffer.char_before(offset)) {
            offset -= 1;
        }
    }
    offset
}

/// Inserts a copy of the line under it, or of the region after it, and moves to the copy,
/// as IntelliJ ⌘D does.
pub fn duplicate_dwim(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    if let Some(region) = buffer.region() {
        let copy = buffer.contents(region.clone());
        let point_at_end = buffer.point() == region.end;
        buffer.apply_edits(vec![(region.end..region.end, copy)]);
        let copied = region.end..region.end + region.len();
        let (mark, point) =
            if point_at_end { (copied.start, copied.end) } else { (copied.end, copied.start) };
        buffer.set_mark(mark);
        buffer.set_point(point);
        buffer.activate_mark();
        return Ok(());
    }
    let line = buffer.text().line_range(buffer.point_line()).ok_or("No line")?;
    let column = buffer.point() - line.start;
    let copy = format!("\n{}", buffer.contents(line.clone()));
    buffer.apply_edits(vec![(line.end..line.end, copy)]);
    buffer.set_point(line.end + 1 + column);
    Ok(())
}

pub fn move_line_up(editor: &mut Editor) -> CommandResult {
    move_lines(editor, false)
}

pub fn move_line_down(editor: &mut Editor) -> CommandResult {
    move_lines(editor, true)
}

/// Swaps the line, or the region's lines, with the line below, or above unless `down`,
/// point and the region going with them.
fn move_lines(editor: &mut Editor, down: bool) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let text = buffer.text();
    let span = buffer.region().unwrap_or_else(|| buffer.point()..buffer.point());
    let (first, last) = lines_of(buffer, span);
    let other = if down { last + 1 } else { first.checked_sub(1).ok_or("Beginning of buffer")? };
    let (Some(block_start), Some(block_end), Some(other_range)) = (
        text.line_start(first),
        text.line_range(last).map(|range| range.end),
        text.line_range(other),
    ) else {
        return Err("End of buffer".into());
    };
    let block = block_start..block_end;
    let shift = other_range.len() as isize + 1;
    let shift = if down { shift } else { -shift };
    let (point, mark, active) = (buffer.point(), buffer.mark(), buffer.is_mark_active());
    let (block_text, other_text) =
        (buffer.contents(block.clone()), buffer.contents(other_range.clone()));
    let (range, moved) = if down {
        (block.start..other_range.end, format!("{other_text}\n{block_text}"))
    } else {
        (other_range.start..block.end, format!("{block_text}\n{other_text}"))
    };
    buffer.apply_edits(vec![(range, moved)]);
    let moved_by = |offset: usize| offset.saturating_add_signed(shift);
    buffer.set_point(moved_by(point));
    if let Some(mark) = mark.filter(|_| active) {
        buffer.set_mark(moved_by(mark));
        buffer.activate_mark();
    }
    Ok(())
}
