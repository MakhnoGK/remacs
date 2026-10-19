use std::ops::Range;

use remacs_core::{Buffer, CharClass, CommandResult, Editor, SyntaxContext};

/// Lines of overlap kept when paging, like Emacs `next-screen-context-lines`.
const CONTEXT_LINES: usize = 2;

pub const RECENTER_TOP_BOTTOM: &str = "recenter-top-bottom";
pub const MOVE_TO_WINDOW_LINE_TOP_BOTTOM: &str = "move-to-window-line-top-bottom";

/// Reaching an end of the buffer is told, not an error: nothing went wrong.
pub(crate) fn at_end_of_buffer(editor: &mut Editor) -> CommandResult {
    editor.set_message("End of buffer");
    Ok(())
}

pub(crate) fn at_beginning_of_buffer(editor: &mut Editor) -> CommandResult {
    editor.set_message("Beginning of buffer");
    Ok(())
}

pub fn forward_char(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    if buffer.point() == buffer.text().len() {
        return at_end_of_buffer(editor);
    }
    buffer.set_point(buffer.text().next_char_boundary(buffer.point()));
    Ok(())
}

pub fn backward_char(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    if buffer.point() == 0 {
        return at_beginning_of_buffer(editor);
    }
    buffer.set_point(buffer.text().previous_char_boundary(buffer.point()));
    Ok(())
}

pub fn forward_word(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    if buffer.point() == buffer.text().len() {
        return at_end_of_buffer(editor);
    }
    buffer.set_point(forward_word_end(buffer, buffer.point()));
    Ok(())
}

pub fn backward_word(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    if buffer.point() == 0 {
        return at_beginning_of_buffer(editor);
    }
    buffer.set_point(backward_word_start(buffer, buffer.point()));
    Ok(())
}

/// Moves to the blank line after the paragraph, like Emacs `forward-paragraph`.
pub fn forward_paragraph(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    if buffer.point() == buffer.text().len() {
        return at_end_of_buffer(editor);
    }
    buffer.set_point(paragraph_end(buffer, buffer.point_line()));
    Ok(())
}

/// Moves to the blank line before the paragraph, like Emacs `backward-paragraph`.
pub fn backward_paragraph(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    if buffer.point() == 0 {
        return at_beginning_of_buffer(editor);
    }
    buffer.set_point(paragraph_start(buffer, buffer.point_line()));
    Ok(())
}

pub fn next_line(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    let next = buffer.point_line() + 1;
    if next >= buffer.text().line_count() {
        return at_end_of_buffer(editor);
    }
    buffer.move_to_line(next);
    Ok(())
}

pub fn previous_line(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    let Some(previous) = buffer.point_line().checked_sub(1) else {
        return at_beginning_of_buffer(editor);
    };
    buffer.move_to_line(previous);
    Ok(())
}

pub fn move_beginning_of_line(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    let line_start = buffer.text().line_start(buffer.point_line()).unwrap_or(0);
    buffer.set_point(line_start);
    Ok(())
}

pub fn move_end_of_line(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    buffer.set_point(line_end(buffer));
    Ok(())
}

/// Like Emacs, it pushes the mark unless the region is active, so `C-u C-SPC` comes back.
pub fn beginning_of_buffer(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    push_mark_unless_selecting(buffer);
    buffer.set_point(0);
    Ok(())
}

pub fn end_of_buffer(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    push_mark_unless_selecting(buffer);
    buffer.set_point(buffer.text().len());
    Ok(())
}

fn push_mark_unless_selecting(buffer: &mut Buffer) {
    if !buffer.is_mark_active() {
        buffer.push_mark();
    }
}

pub fn scroll_up_command(editor: &mut Editor) -> CommandResult {
    if scroll_page(editor, 1) {
        Ok(())
    } else {
        at_end_of_buffer(editor)
    }
}

pub fn scroll_down_command(editor: &mut Editor) -> CommandResult {
    if scroll_page(editor, -1) {
        Ok(())
    } else {
        at_beginning_of_buffer(editor)
    }
}

pub fn recenter(editor: &mut Editor) -> CommandResult {
    let point_line = editor.window_buffer().point_line();
    editor.viewport.center_on(point_line);
    Ok(())
}

/// Centers the line with point, then, run again, puts it at the top, then at the bottom.
pub fn recenter_top_bottom(editor: &mut Editor) -> CommandResult {
    let point_line = editor.window_buffer().point_line();
    match cycle_step(editor, RECENTER_TOP_BOTTOM, 3) {
        0 => editor.viewport.center_on(point_line),
        1 => editor.viewport.put_at_top(point_line),
        _ => editor.viewport.put_at_bottom(point_line),
    }
    Ok(())
}

/// Moves point to the start of the window's middle line, then, run again, of its top line,
/// then of its bottom one, out of the margins, like Emacs `move-to-window-line-top-bottom`.
pub fn move_to_window_line_top_bottom(editor: &mut Editor) -> CommandResult {
    let viewport = editor.viewport;
    let line = match cycle_step(editor, MOVE_TO_WINDOW_LINE_TOP_BOTTOM, 3) {
        0 => viewport.first_line + viewport.height / 2,
        1 => viewport.first_line,
        _ => viewport.last_line(),
    };
    let buffer = editor.window_buffer_mut();
    let line = viewport.clamp(line, buffer.text().line_count());
    let line_start = buffer
        .text()
        .line_start(line)
        .unwrap_or(buffer.text().len());
    buffer.set_point(line_start);
    Ok(())
}

#[derive(Default)]
struct CycleStep(usize);

/// Where a command that cycles is: 0 the first time, one more each time it runs again
/// right after itself, back to 0 after `steps`.
pub(crate) fn cycle_step(editor: &mut Editor, command: &'static str, steps: usize) -> usize {
    let again = editor.last_command() == Some(command);
    let step = &mut editor.plugin_state::<CycleStep>().0;
    *step = if again { (*step + 1) % steps } else { 0 };
    *step
}

pub(crate) fn line_end(buffer: &Buffer) -> usize {
    buffer
        .text()
        .line_range(buffer.point_line())
        .map_or(buffer.text().len(), |range| range.end)
}

// Word steps follow IntelliJ's default macOS caret stops (`EditorActionUtil`): moves stop
// at the near end of a token and cross one line break at a time; kills stop at the line
// ends and take a quoted word with its quotes.

/// Where `forward-word` goes: the end of the next token, or the start of the next line.
pub(crate) fn forward_word_end(buffer: &Buffer, offset: usize) -> usize {
    next_stop(
        buffer,
        offset,
        next_line_start(buffer, offset),
        is_token_end,
    )
}

/// Where `backward-word` goes: the start of the previous token, or the end of the previous
/// line.
pub(crate) fn backward_word_start(buffer: &Buffer, offset: usize) -> usize {
    previous_stop(
        buffer,
        offset,
        previous_line_end(buffer, offset),
        is_token_start,
    )
}

/// Where `kill-word` kills to: the next start or end of a token, past a quoted word, or the
/// end of the line, or from there the start of the next.
pub(crate) fn kill_word_end(editor: &Editor, offset: usize) -> usize {
    let buffer = editor.buffer();
    let line_end = line_range_at(buffer, offset).end;
    let limit = if offset < line_end {
        line_end
    } else {
        next_line_start(buffer, offset)
    };
    let stop = next_stop(buffer, offset, limit, is_token_edge);
    if let Some(quote) = buffer.char_before(stop).filter(|&c| is_quote(c))
        && stop < limit
        && opens_quote(editor, stop, quote)
    {
        let before_quote = next_stop(buffer, stop, limit, is_token_edge);
        if buffer.char_after(before_quote) == Some(quote) {
            return next_stop(buffer, before_quote, limit, is_token_edge);
        }
    }
    stop
}

/// Where `backward-kill-word` kills from: the previous start of a token, before a quoted
/// word, or the start of the line, or from there the end of the previous.
pub(crate) fn backward_kill_word_start(editor: &Editor, offset: usize) -> usize {
    let buffer = editor.buffer();
    let line_start = line_range_at(buffer, offset).start;
    let limit = if offset > line_start {
        line_start
    } else {
        previous_line_end(buffer, offset)
    };
    let stop = previous_stop(buffer, offset, limit, is_token_start);
    if let Some(quote) = buffer.char_after(stop).filter(|&c| is_quote(c))
        && stop > limit
    {
        let after_quote = previous_stop(buffer, stop, limit, is_token_edge);
        if buffer.char_before(after_quote) == Some(quote) && opens_quote(editor, after_quote, quote)
        {
            return previous_stop(buffer, after_quote, limit, is_token_start);
        }
    }
    stop
}

/// Whether the `quote` ending at `offset` starts a string, rather than sitting inside one
/// or in a comment or text no parser reads.
fn opens_quote(editor: &Editor, offset: usize, quote: char) -> bool {
    editor.syntax_at(offset - quote.len_utf8()) != SyntaxContext::String
        && editor.syntax_at(offset) == SyntaxContext::String
}

/// The first offset after `offset`, up to `limit`, where `stops` holds, or `limit`.
fn next_stop(buffer: &Buffer, mut offset: usize, limit: usize, stops: StopAt) -> usize {
    while offset < limit {
        offset = buffer.text().next_char_boundary(offset);
        if stops(buffer, offset) {
            break;
        }
    }
    offset
}

/// The last offset before `offset`, down to `limit`, where `stops` holds, or `limit`.
fn previous_stop(buffer: &Buffer, mut offset: usize, limit: usize, stops: StopAt) -> usize {
    while offset > limit {
        offset = buffer.text().previous_char_boundary(offset);
        if stops(buffer, offset) {
            break;
        }
    }
    offset
}

type StopAt = fn(&Buffer, usize) -> bool;

fn is_token_start(buffer: &Buffer, offset: usize) -> bool {
    match (buffer.char_before(offset), buffer.char_after(offset)) {
        (_, None) => false,
        (_, Some(after)) if after.is_whitespace() => false,
        (None, _) => true,
        (Some(before), Some(after)) => splits(before, after),
    }
}

fn is_token_end(buffer: &Buffer, offset: usize) -> bool {
    match (buffer.char_before(offset), buffer.char_after(offset)) {
        (None, _) => false,
        (Some(before), _) if before.is_whitespace() => false,
        (_, None) => true,
        (Some(before), Some(after)) => splits(before, after),
    }
}

fn is_token_edge(buffer: &Buffer, offset: usize) -> bool {
    is_token_start(buffer, offset) || is_token_end(buffer, offset)
}

/// Whether a token ends between two characters next to each other: a word next to
/// anything else, a quote, or a bracket, `,` or `;` next to another character; other
/// punctuation runs together, as in `->` or `!=`.
/// shortcut: character rules stand in for the lexer tokens IntelliJ steps by; take the
/// syntax tree's leaves if a language splits wrong.
fn splits(before: char, after: char) -> bool {
    if CharClass::of(before) != CharClass::of(after) {
        return true;
    }
    CharClass::of(before) == CharClass::Punctuation
        && (is_quote(before)
            || is_quote(after)
            || (stands_alone(before) || stands_alone(after)) && before != after)
}

fn is_quote(character: char) -> bool {
    matches!(character, '"' | '\'' | '`')
}

fn stands_alone(character: char) -> bool {
    matches!(character, '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';')
}

fn line_range_at(buffer: &Buffer, offset: usize) -> Range<usize> {
    let text = buffer.text();
    text.line_range(text.line_of(offset))
        .unwrap_or(offset..offset)
}

fn next_line_start(buffer: &Buffer, offset: usize) -> usize {
    let text = buffer.text();
    text.line_start(text.line_of(offset) + 1)
        .unwrap_or(text.len())
}

fn previous_line_end(buffer: &Buffer, offset: usize) -> usize {
    let text = buffer.text();
    text.line_of(offset)
        .checked_sub(1)
        .and_then(|line| text.line_range(line))
        .map_or(0, |range| range.end)
}

/// The start of the first blank line after the paragraph at or after `line`, or the end
/// of the buffer.
pub(crate) fn paragraph_end(buffer: &Buffer, mut line: usize) -> usize {
    let line_count = buffer.text().line_count();
    while line < line_count && is_blank_line(buffer, line) {
        line += 1;
    }
    while line < line_count && !is_blank_line(buffer, line) {
        line += 1;
    }
    buffer
        .text()
        .line_start(line)
        .unwrap_or(buffer.text().len())
}

/// The start of the last blank line before the paragraph at or before `line`, or the
/// beginning of the buffer.
pub(crate) fn paragraph_start(buffer: &Buffer, line: usize) -> usize {
    let mut lines = (0..=line)
        .rev()
        .skip_while(|&line| is_blank_line(buffer, line));
    lines
        .find(|&line| is_blank_line(buffer, line))
        .and_then(|line| buffer.text().line_start(line))
        .unwrap_or(0)
}

/// Whether the line holds only whitespace.
fn is_blank_line(buffer: &Buffer, line: usize) -> bool {
    let Some(range) = buffer.text().line_range(line) else {
        return true;
    };
    let mut offset = range.start;
    while offset < range.end {
        match buffer.char_after(offset) {
            Some(character) if character.is_whitespace() => {
                offset = buffer.text().next_char_boundary(offset);
            }
            _ => return false,
        }
    }
    true
}

fn scroll_page(editor: &mut Editor, direction: isize) -> bool {
    let page = editor.viewport.height.saturating_sub(CONTEXT_LINES).max(1) as isize;
    let first_line = editor.viewport.first_line;
    editor.scroll(direction * page);
    editor.viewport.first_line != first_line
}
