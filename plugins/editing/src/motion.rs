use remacs_core::{Buffer, CommandResult, Editor};

/// Lines of overlap kept when paging, like Emacs `next-screen-context-lines`.
const CONTEXT_LINES: usize = 2;

pub fn forward_char(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    if buffer.point() == buffer.text().len() {
        return Err("End of buffer".into());
    }
    buffer.set_point(buffer.text().next_char_boundary(buffer.point()));
    Ok(())
}

pub fn backward_char(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    if buffer.point() == 0 {
        return Err("Beginning of buffer".into());
    }
    buffer.set_point(buffer.text().previous_char_boundary(buffer.point()));
    Ok(())
}

pub fn forward_word(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    if buffer.point() == buffer.text().len() {
        return Err("End of buffer".into());
    }
    buffer.set_point(word_end(buffer, buffer.point()));
    Ok(())
}

pub fn backward_word(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    if buffer.point() == 0 {
        return Err("Beginning of buffer".into());
    }
    buffer.set_point(word_start(buffer, buffer.point()));
    Ok(())
}

pub fn next_line(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    let next = buffer.point_line() + 1;
    if next >= buffer.text().line_count() {
        return Err("End of buffer".into());
    }
    buffer.move_to_line(next);
    Ok(())
}

pub fn previous_line(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    let previous = buffer
        .point_line()
        .checked_sub(1)
        .ok_or("Beginning of buffer")?;
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
    scroll_page(editor, 1)
        .then_some(())
        .ok_or("End of buffer".into())
}

pub fn scroll_down_command(editor: &mut Editor) -> CommandResult {
    scroll_page(editor, -1)
        .then_some(())
        .ok_or("Beginning of buffer".into())
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

/// End of the word at or after `offset`, like Emacs `forward-word`.
pub(crate) fn word_end(buffer: &Buffer, mut offset: usize) -> usize {
    let mut in_word = false;
    while let Some(character) = buffer.char_after(offset) {
        let is_word = character.is_alphanumeric();
        if in_word && !is_word {
            break;
        }
        in_word |= is_word;
        offset = buffer.text().next_char_boundary(offset);
    }
    offset
}

/// Start of the word at or before `offset`, like Emacs `backward-word`.
pub(crate) fn word_start(buffer: &Buffer, mut offset: usize) -> usize {
    let mut in_word = false;
    while let Some(character) = buffer.char_before(offset) {
        let is_word = character.is_alphanumeric();
        if in_word && !is_word {
            break;
        }
        in_word |= is_word;
        offset = buffer.text().previous_char_boundary(offset);
    }
    offset
}

fn scroll_page(editor: &mut Editor, direction: isize) -> bool {
    let page = editor.viewport.height.saturating_sub(CONTEXT_LINES).max(1) as isize;
    let first_line = editor.viewport.first_line;
    editor.scroll(direction * page);
    editor.viewport.first_line != first_line
}
