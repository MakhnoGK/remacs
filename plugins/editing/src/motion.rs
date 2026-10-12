use remacs_core::{Buffer, CharClass, CommandResult, Editor};

/// Lines of overlap kept when paging, like Emacs `next-screen-context-lines`.
const CONTEXT_LINES: usize = 2;

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
    buffer.set_point(word_end(buffer, buffer.point()));
    Ok(())
}

pub fn backward_word(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    if buffer.point() == 0 {
        return at_beginning_of_buffer(editor);
    }
    buffer.set_point(word_start(buffer, buffer.point()));
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

pub fn beginning_of_buffer(editor: &mut Editor) -> CommandResult {
    editor.buffer_mut().set_point(0);
    Ok(())
}

pub fn end_of_buffer(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    buffer.set_point(buffer.text().len());
    Ok(())
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

pub(crate) fn line_end(buffer: &Buffer) -> usize {
    buffer
        .text()
        .line_range(buffer.point_line())
        .map_or(buffer.text().len(), |range| range.end)
}

/// End of the step after `offset`: a word with the whitespace to its left on its line, or
/// a run of punctuation, or whitespace on its own before punctuation or across lines.
pub(crate) fn word_end(buffer: &Buffer, offset: usize) -> usize {
    let Some(first) = buffer.char_after(offset).map(CharClass::of) else {
        return offset;
    };
    let end = skip_forward(buffer, offset, first);
    if first == CharClass::Space
        && buffer.char_after(end).map(CharClass::of) == Some(CharClass::Word)
        && !buffer.contents(offset..end).contains('\n')
    {
        skip_forward(buffer, end, CharClass::Word)
    } else {
        end
    }
}

/// Start of the step before `offset`: a word with the whitespace to its left, or a run
/// of punctuation or of whitespace.
pub(crate) fn word_start(buffer: &Buffer, offset: usize) -> usize {
    let Some(last) = buffer.char_before(offset).map(CharClass::of) else {
        return offset;
    };
    let start = skip_backward(buffer, offset, last);
    if last == CharClass::Word {
        skip_backward(buffer, start, CharClass::Space)
    } else {
        start
    }
}

fn skip_forward(buffer: &Buffer, mut offset: usize, class: CharClass) -> usize {
    while buffer.char_after(offset).map(CharClass::of) == Some(class) {
        offset = buffer.text().next_char_boundary(offset);
    }
    offset
}

fn skip_backward(buffer: &Buffer, mut offset: usize, class: CharClass) -> usize {
    while buffer.char_before(offset).map(CharClass::of) == Some(class) {
        offset = buffer.text().previous_char_boundary(offset);
    }
    offset
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
