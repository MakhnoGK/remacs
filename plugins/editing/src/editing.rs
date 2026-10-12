use remacs_core::{Buffer, CommandError, CommandResult, Editor};

use crate::motion::{at_beginning_of_buffer, at_end_of_buffer};
use crate::{indent, pairs};

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
    let pairs = pairs::mode_pairs(editor);
    let context = editor.syntax_at(editor.buffer().point());
    let buffer = writable_buffer(editor)?;
    if pairs::type_paired(buffer, pairs, context, character) {
        return Ok(());
    }
    indent::indent_for_closer(buffer, character);
    buffer.insert(character.encode_utf8(&mut [0; 4]));
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
        return at_end_of_buffer(editor);
    }
    let next = buffer.text().next_char_boundary(point);
    buffer.delete(point..next);
    Ok(())
}

pub fn delete_backward_char(editor: &mut Editor) -> CommandResult {
    let pairs = pairs::mode_pairs(editor);
    let buffer = writable_buffer(editor)?;
    if let Some(region) = buffer
        .region()
        .or_else(|| pairs::empty_pair_around(buffer, pairs))
    {
        buffer.delete(region);
        return Ok(());
    }
    let point = buffer.point();
    if point == 0 {
        return at_beginning_of_buffer(editor);
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
