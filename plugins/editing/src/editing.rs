use remacs_core::{Buffer, CommandError, CommandResult, Editor};

const TAB_WIDTH: usize = 4;

/// The buffer to edit, unless it is read-only.
pub fn writable_buffer(editor: &mut Editor) -> Result<&mut Buffer, CommandError> {
    let buffer = editor.buffer_mut();
    buffer.ensure_writable()?;
    Ok(buffer)
}

pub fn self_insert(editor: &mut Editor) -> CommandResult {
    let character = editor
        .last_key()
        .and_then(|key| key.printable_char())
        .ok_or("Not a printable key")?;
    writable_buffer(editor)?.insert(character.encode_utf8(&mut [0; 4]));
    Ok(())
}

/// Breaks the line and repeats the current line's indentation.
pub fn newline(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let line_start = buffer.text().line_start(buffer.point_line()).unwrap_or(0);
    let before_point = buffer.text().bytes_in(line_start..buffer.point());
    let indentation_len = before_point
        .iter()
        .take_while(|&&byte| byte == b' ' || byte == b'\t')
        .count();
    let indentation = String::from_utf8_lossy(&before_point[..indentation_len]).into_owned();
    buffer.insert(&format!("\n{indentation}"));
    Ok(())
}

pub fn tab_to_tab_stop(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let spaces = TAB_WIDTH - buffer.point_column() % TAB_WIDTH;
    buffer.insert(&" ".repeat(spaces));
    Ok(())
}

/// Deletes the next character, or the active region without saving it, as in Emacs.
pub fn delete_char(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    if let Some(region) = buffer.region() {
        buffer.delete(region);
        return Ok(());
    }
    let point = buffer.point();
    if point == buffer.text().len() {
        return Err("End of buffer".into());
    }
    let next = buffer.text().next_char_boundary(point);
    buffer.delete(point..next);
    Ok(())
}

pub fn delete_backward_char(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    if let Some(region) = buffer.region() {
        buffer.delete(region);
        return Ok(());
    }
    let point = buffer.point();
    if point == 0 {
        return Err("Beginning of buffer".into());
    }
    let previous = buffer.text().previous_char_boundary(point);
    buffer.delete(previous..point);
    Ok(())
}

pub fn undo(editor: &mut Editor) -> CommandResult {
    if !writable_buffer(editor)?.undo() {
        return Err("No further undo information".into());
    }
    editor.set_message("Undo");
    Ok(())
}

pub fn undo_redo(editor: &mut Editor) -> CommandResult {
    if !writable_buffer(editor)?.redo() {
        return Err("No further redo information".into());
    }
    editor.set_message("Redo");
    Ok(())
}

pub fn keyboard_quit(editor: &mut Editor) -> CommandResult {
    editor.buffer_mut().deactivate_mark();
    editor.set_message("Quit");
    Ok(())
}
