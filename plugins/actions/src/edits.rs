use std::path::Path;

use remacs_core::{CommandResult, Editor, FileEdits};

use crate::menu::{Awaited, state};

/// Makes the edits of the action chosen from list `id`, opening files that are not
/// visited yet; they are left unsaved. Edits that come after another command are
/// refused, since the text they were made for may have changed.
pub fn apply(editor: &mut Editor, id: u64, changes: Vec<FileEdits>) -> CommandResult {
    if state(editor).awaited != Some(Awaited::Apply(id)) {
        return Err("Code action dropped: another command ran since".into());
    }
    let shown = editor.window_buffer().path().map(Path::to_path_buf);
    let mut others = Vec::new();
    for edits in changes {
        let path = edits.path.clone();
        let buffer = editor
            .edit_file(edits)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if buffer.path() != shown.as_deref() {
            others.push(buffer.name().to_owned());
        }
    }
    if !others.is_empty() {
        editor.set_message(format!("Also edited {}", others.join(", ")));
    }
    Ok(())
}

/// Tells the user that the action chosen from list `id` could not be carried out.
pub fn failed(editor: &mut Editor, id: u64, message: &str) {
    if state(editor).awaited == Some(Awaited::Apply(id)) {
        editor.set_error(format!("Code action failed: {message}"));
    }
}
