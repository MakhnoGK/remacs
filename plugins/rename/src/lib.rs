//! Built-in plugin: renames the symbol at point everywhere it is used, like Emacs
//! `eglot-rename`, saving every file it changes. It knows nothing of where the changes
//! come from; backends such as `lsp` take its requests and deliver the symbol's name and
//! the changes renaming it makes.

use std::mem;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_core::{
    ChangedFiles, Command, CommandResult, Completions, Editor, FileChange, MenuItem, Plugin,
    Prompt, Registry, visiting_file,
};

pub struct RenamePlugin;

const RENAME_SYMBOL: &str = "rename-symbol";
const NAME_CATEGORY: &str = "rename-symbol";

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(RENAME_SYMBOL, "Rename the symbol at point everywhere it is used", rename_symbol),
];

const BINDINGS: &[(&str, &str)] = &[("C-c C-r", RENAME_SYMBOL)];

const MENU_ITEM: MenuItem =
    MenuItem::new("Edit", "code", "Rename Symbol…", RENAME_SYMBOL).enabled_when(visiting_file);

impl Plugin for RenamePlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.push(MENU_ITEM);
    }
}

/// What backends are asked about the symbol at `offset` of `path`'s buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Its name, to edit into the new one; answered with [`deliver_name`].
    Name { id: u64, path: PathBuf, offset: usize },
    /// The changes renaming it to `new_name` makes; answered with [`deliver_changes`].
    Rename { id: u64, path: PathBuf, offset: usize, new_name: String },
}

#[derive(Default)]
struct State {
    next_id: u64,
    requests: Vec<Request>,
    awaited: Option<Awaited>,
}

/// The newest request; answers to older ones are dropped.
struct Awaited {
    id: u64,
    path: PathBuf,
    offset: usize,
    /// While renaming, the old name and the new one.
    names: Option<(String, String)>,
}

fn rename_symbol(editor: &mut Editor) -> CommandResult {
    if editor.minibuffer().is_some() {
        return Err("Renaming works in buffers only".into());
    }
    let buffer = editor.window_buffer();
    let path = buffer.path().ok_or("Not visiting a file")?.to_path_buf();
    let offset = buffer.point();
    let id = ask(editor, |id| Request::Name { id, path: path.clone(), offset });
    editor.plugin_state::<State>().awaited = Some(Awaited { id, path, offset, names: None });
    Ok(())
}

fn ask(editor: &mut Editor, request: impl FnOnce(u64) -> Request) -> u64 {
    let state = editor.plugin_state::<State>();
    state.next_id += 1;
    let id = state.next_id;
    state.requests.push(request(id));
    id
}

pub fn take_requests(editor: &mut Editor) -> Vec<Request> {
    mem::take(&mut editor.plugin_state::<State>().requests)
}

/// Answers request `id` with the symbol's name, or why it cannot be renamed. The new name
/// is read only while point stays where the command left it.
pub fn deliver_name(editor: &mut Editor, id: u64, name: Result<String, String>) {
    let Some(awaited) = take_awaited(editor, id, false) else {
        return;
    };
    let name = match name {
        Ok(name) => name,
        Err(error) => return editor.set_error(error),
    };
    let buffer = editor.window_buffer();
    let moved = buffer.path() != Some(awaited.path.as_path()) || buffer.point() != awaited.offset;
    if moved || editor.minibuffer().is_some() {
        return;
    }
    let result = read_new_name(editor, awaited, name);
    if let Err(error) = result {
        editor.set_error(error.0);
    }
}

fn read_new_name(editor: &mut Editor, awaited: Awaited, name: String) -> CommandResult {
    let old_name = name.clone();
    let prompt = Prompt {
        label: "Rename to".into(),
        category: NAME_CATEGORY,
        initial_input: name.clone(),
        hints: &[("↵", "Rename"), ("C-g", "Cancel")],
        check: Some(Rc::new(move |input: &str| {
            if input.trim().is_empty() {
                Err("Type the new name".into())
            } else if input == old_name {
                Err(format!("It is {old_name} already"))
            } else {
                Ok(())
            }
        })),
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        move |editor, new_name| {
            let Awaited { path, offset, .. } = awaited;
            let id = ask(editor, |id| Request::Rename {
                id,
                path: path.clone(),
                offset,
                new_name: new_name.clone(),
            });
            editor.plugin_state::<State>().awaited =
                Some(Awaited { id, path, offset, names: Some((name, new_name)) });
            editor.set_message("Renaming…");
            Ok(())
        },
    )
}

/// Answers request `id` with the changes renaming makes, or why it cannot be done; makes
/// them and saves every file changed.
pub fn deliver_changes(editor: &mut Editor, id: u64, changes: Result<Vec<FileChange>, String>) {
    let Some(Awaited { names: Some((from, to)), .. }) = take_awaited(editor, id, true) else {
        return;
    };
    let changes = match changes {
        Ok(changes) if changes.is_empty() => Err(format!("Nothing to rename for {from}")),
        changes => changes,
    };
    let places = changes.as_ref().map_or(0, |changes| {
        changes
            .iter()
            .map(|change| match change {
                FileChange::Edit(edits) => edits.edits.len(),
                _ => 0,
            })
            .sum()
    });
    match changes.and_then(|changes| editor.change_files(changes)) {
        Ok(changed) => editor.set_done(renamed_message(&from, &to, places, &changed)),
        Err(error) => editor.set_error(format!("Could not rename {from}: {error}")),
    }
}

fn take_awaited(editor: &mut Editor, id: u64, renaming: bool) -> Option<Awaited> {
    let state = editor.plugin_state::<State>();
    let awaited = state.awaited.as_ref()?;
    if awaited.id != id || awaited.names.is_some() != renaming {
        return None;
    }
    state.awaited.take()
}

/// `Renamed a to b: 5 places in 2 files; moved a.rs to b.rs`.
fn renamed_message(from: &str, to: &str, places: usize, changed: &ChangedFiles) -> String {
    let mut message = format!("Renamed {from} to {to}: {places} {}", plural(places, "place"));
    let files = changed.edited.len();
    if files > 1 {
        message.push_str(&format!(" in {files} files"));
    }
    for (old, new) in &changed.moved {
        message.push_str(&format!("; moved {} to {}", file_name(old), file_name(new)));
    }
    message
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 { word.to_owned() } else { format!("{word}s") }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests;
