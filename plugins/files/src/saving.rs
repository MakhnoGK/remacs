use remacs_core::{CommandResult, Editor};

use crate::buffers::has_unsaved_changes;

pub fn save_buffer(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    if buffer.path().is_none() {
        return Err(format!("Buffer {} is not visiting a file", buffer.name()).into());
    }
    if !buffer.is_modified() {
        editor.set_message("(No changes need to be saved)");
        return Ok(());
    }
    let name = buffer.name().to_owned();
    save(editor, &name)
}

/// Offers to save each file with unsaved changes.
pub fn save_some_buffers(editor: &mut Editor) -> CommandResult {
    let names = unsaved_files(editor);
    if names.is_empty() {
        editor.set_message("(No files need saving)");
        return Ok(());
    }
    offer_to_save(editor, names, |_| Ok(()))
}

/// Offers to save each unsaved file, then quits, asking again if some stay unsaved.
pub fn save_buffers_kill_terminal(editor: &mut Editor) -> CommandResult {
    let names = unsaved_files(editor);
    offer_to_save(editor, names, |editor| {
        if unsaved_files(editor).is_empty() {
            editor.request_quit();
            return Ok(());
        }
        editor.ask_y_or_n("Modified buffers exist; exit anyway?", |editor, yes| {
            if yes {
                editor.request_quit();
            }
            Ok(())
        })
    })
}

/// Asks about the buffers one by one, then continues with `then`.
fn offer_to_save(
    editor: &mut Editor,
    mut names: Vec<String>,
    then: impl FnOnce(&mut Editor) -> CommandResult + 'static,
) -> CommandResult {
    if names.is_empty() {
        return then(editor);
    }
    let name = names.remove(0);
    let path = editor
        .buffers()
        .iter()
        .find(|buffer| buffer.name() == name)
        .and_then(|buffer| buffer.path())
        .map_or_else(|| name.clone(), |path| path.display().to_string());
    editor.ask_y_or_n(format!("Save file {path}?"), move |editor, yes| {
        if yes {
            save(editor, &name)?;
        }
        offer_to_save(editor, names, then)
    })
}

fn save(editor: &mut Editor, name: &str) -> CommandResult {
    let buffer = editor
        .buffer_named_mut(name)
        .ok_or_else(|| format!("No buffer named {name}"))?;
    buffer.save()?;
    let path = buffer.path().map(|path| path.display().to_string());
    editor.set_message(format!("Wrote {}", path.unwrap_or_default()));
    Ok(())
}

fn unsaved_files(editor: &Editor) -> Vec<String> {
    editor
        .buffers()
        .iter()
        .map(|buffer| buffer.name().to_owned())
        .filter(|name| has_unsaved_changes(editor, name))
        .collect()
}
