use std::fs;
use std::path::Path;

use remacs_core::{CommandResult, Count, Editor, Prompt, quote};

use crate::buffers::has_unsaved_changes;
use crate::find_file::{FILE_CATEGORY, directory_candidates};
use crate::project::{abbreviate_home, default_directory, expand_home, with_slash};
use crate::session;

/// Saves the buffer in its file, asking for a file name if it has none.
pub fn save_buffer(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    if buffer.path().is_none() {
        return write_file(editor);
    }
    if !buffer.is_modified() {
        editor.set_message("(No changes need to be saved)");
        return Ok(());
    }
    let name = buffer.name().to_owned();
    save_unless_superseded(editor, name, |_| Ok(()))
}

/// Saves the buffer under a new file name and visits that file from then on; a directory
/// gets a file named after the buffer.
pub fn write_file(editor: &mut Editor) -> CommandResult {
    let directory = default_directory(editor);
    let prompt = Prompt {
        label: "Write file".into(),
        category: FILE_CATEGORY,
        initial_input: with_slash(&abbreviate_home(&directory)),
        count: Count::Matches,
        wide: true,
        hints: &[
            ("↵", "Write"),
            ("M-↵", "Write as typed"),
            ("⇥", "Complete path"),
            ("C-g", "Cancel"),
        ],
        ..Prompt::default()
    };
    let base = directory.clone();
    editor.read(
        prompt,
        move |_, input| directory_candidates(&base, input),
        move |editor, answer| {
            let mut path = directory.join(expand_home(&answer));
            if path.is_dir() {
                path.push(editor.window_buffer().name());
            }
            let current_file = fs::canonicalize(&path).ok();
            if path.exists() && current_file.as_deref() != editor.window_buffer().path() {
                let question =
                    format!("File {} exists; overwrite?", quote(&abbreviate_home(&path)));
                return editor.ask_y_or_n(question, move |editor, yes| {
                    if yes { write_to(editor, &path) } else { Ok(()) }
                });
            }
            write_to(editor, &path)
        },
    )
}

fn write_to(editor: &mut Editor, path: &Path) -> CommandResult {
    editor.set_visited_file(path)?;
    let name = editor.window_buffer().name().to_owned();
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

/// Offers to save each unsaved file, then quits, asking again if some stay unsaved or
/// programs still run.
pub fn save_buffers_kill_terminal(editor: &mut Editor) -> CommandResult {
    let names = unsaved_files(editor);
    offer_to_save(editor, names, |editor| {
        if unsaved_files(editor).is_empty() {
            return quit_ending_programs(editor);
        }
        editor.ask_y_or_n("Modified buffers exist; exit anyway?", |editor, yes| {
            if yes { quit_ending_programs(editor) } else { Ok(()) }
        })
    })
}

fn quit_ending_programs(editor: &mut Editor) -> CommandResult {
    let running = editor.running_programs();
    if running.is_empty() {
        session::quit(editor);
        return Ok(());
    }
    let names: Vec<String> = running.iter().map(|name| quote(name)).collect();
    let question = format!("Programs run in {}; kill them and exit anyway?", names.join(", "));
    editor.ask_y_or_n(question, |editor, yes| {
        if yes {
            session::quit(editor);
        }
        Ok(())
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
        .map_or_else(|| name.clone(), abbreviate_home);
    editor.ask_y_or_n(format!("Save file {}?", quote(&path)), move |editor, yes| {
        if yes {
            return save_unless_superseded(editor, name, |editor| {
                offer_to_save(editor, names, then)
            });
        }
        offer_to_save(editor, names, then)
    })
}

/// Saves, first asking if the file changed on disk or went away since it was read or
/// saved, then continues with `then`.
fn save_unless_superseded(
    editor: &mut Editor,
    name: String,
    then: impl FnOnce(&mut Editor) -> CommandResult + 'static,
) -> CommandResult {
    let buffer = editor
        .buffers()
        .iter()
        .find(|buffer| buffer.name() == name)
        .ok_or_else(|| format!("No buffer named {name}"))?;
    let path = buffer.path().ok_or("The buffer visits no file")?;
    let text = buffer.text();
    // A text never read nor saved was for a new file, which someone may have made since.
    let superseded = match text.path() {
        Some(_) => text.changed_on_disk().unwrap_or(false),
        None => path.exists(),
    };
    if !superseded {
        save(editor, &name)?;
        return then(editor);
    }
    let question = if path.exists() {
        format!("{} changed on disk; save over it?", quote(&abbreviate_home(path)))
    } else {
        format!("{} was deleted; save it again?", quote(&abbreviate_home(path)))
    };
    editor.ask_y_or_n(question, move |editor, yes| {
        if yes {
            save(editor, &name)?;
        }
        then(editor)
    })
}

fn save(editor: &mut Editor, name: &str) -> CommandResult {
    let buffer = editor.buffer_named_mut(name).ok_or_else(|| format!("No buffer named {name}"))?;
    buffer.save()?;
    let path = buffer.path().map(|path| path.display().to_string());
    editor.set_done(format!("Wrote {}", path.unwrap_or_default()));
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
