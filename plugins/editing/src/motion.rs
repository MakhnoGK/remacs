use remacs_core::{Buffer, CharClass, CommandResult, Editor};

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
