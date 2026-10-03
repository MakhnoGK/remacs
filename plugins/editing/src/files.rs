use remacs_core::{CommandResult, Editor};

pub const SAVE_BUFFERS_KILL_TERMINAL: &str = "save-buffers-kill-terminal";

pub fn save_buffer(editor: &mut Editor) -> CommandResult {
    if !editor.buffer.is_modified() {
        editor.set_message("(No changes need to be saved)");
        return Ok(());
    }
    editor.buffer.save()?;
    let path = editor.buffer.path().map(|path| path.display().to_string());
    editor.set_message(format!("Wrote {}", path.unwrap_or_default()));
    Ok(())
}

/// Quits; a modified buffer needs the command twice in a row until prompts exist.
pub fn save_buffers_kill_terminal(editor: &mut Editor) -> CommandResult {
    let repeated = editor.last_command() == Some(SAVE_BUFFERS_KILL_TERMINAL);
    if editor.buffer.is_modified() && !repeated {
        return Err("Buffer modified: C-x C-s to save, or repeat to quit without saving".into());
    }
    editor.request_quit();
    Ok(())
}
