//! Asks servers what renaming files calls for elsewhere, such as the `mod` declarations
//! of renamed Rust modules (`workspace/willRenameFiles`), for files renamed or moved in
//! dired and those a symbol's rename moves, and tells them once done
//! (`workspace/didRenameFiles`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use lsp_types::Uri;
use remacs_core::{Editor, FileChange, FileEdits};

use crate::LanguageServer;
use crate::client::{Client, Outgoing};
use crate::document::Clients;
use crate::uri;
use crate::workspace_edit::file_edits;

/// Edits collected for each question until every server asked has answered.
#[derive(Default)]
struct Asked {
    last_key: u64,
    by_key: HashMap<u64, Collected>,
}

struct Collected {
    asker: Asker,
    unanswered: Vec<Part>,
    edits: Vec<FileEdits>,
}

/// Who the edits go to.
pub(crate) enum Asker {
    /// Dired's question `id`.
    Dired(u64),
    /// The rename plugin's request `id`, whose `changes` rename files: rust-analyzer then
    /// leaves the edits to the references out of them, for `willRenameFiles` to give.
    SymbolRename { id: u64, changes: Vec<FileChange> },
}

/// What one server is asked.
struct Part {
    client: Rc<Client>,
    files: Vec<(Uri, Uri)>,
    /// It answered while still loading the project, so it is asked again once done.
    ask_again: bool,
}

/// Hands answers over, sends dired's new questions and tells servers of files renamed.
pub(crate) fn sync(editor: &mut Editor, servers: &'static [LanguageServer]) {
    collect_answers(editor);
    ask_settled_servers_again(editor);
    for request in remacs_dired::take_rename_requests(editor) {
        ask(editor, servers, &request.renames, Asker::Dired(request.id));
    }
    let moved = editor.take_moved_files();
    for part in by_server(editor, servers, &moved, |(_, to)| to) {
        part.client.send(Outgoing::DidRename { files: part.files });
    }
}

/// Asks each server about the renames, from and to, of the files it serves, starting it
/// if needed, and of directories in the projects it already serves; with none, answers
/// at once.
pub(crate) fn ask(
    editor: &mut Editor,
    servers: &'static [LanguageServer],
    renames: &[(PathBuf, PathBuf)],
    asker: Asker,
) {
    let unanswered = by_server(editor, servers, renames, |(from, _)| from);
    if unanswered.is_empty() {
        return answer(editor, asker, Vec::new());
    }
    let asked = editor.plugin_state::<Asked>();
    asked.last_key += 1;
    let key = asked.last_key;
    for part in &unanswered {
        part.client.send(Outgoing::WillRename {
            id: key,
            files: part.files.clone(),
        });
    }
    let collected = Collected {
        asker,
        unanswered,
        edits: Vec::new(),
    };
    asked.by_key.insert(key, collected);
}

fn answer(editor: &mut Editor, asker: Asker, edits: Vec<FileEdits>) {
    match asker {
        Asker::Dired(id) => remacs_dired::deliver_rename_edits(editor, id, edits),
        Asker::SymbolRename { id, changes } => {
            let edits = edits.into_iter().map(FileChange::Edit);
            remacs_rename::deliver_changes(editor, id, Ok(edits.chain(changes).collect()));
        }
    }
}

/// The renames, from and to, for each server of the files `served` names in them.
fn by_server(
    editor: &mut Editor,
    servers: &'static [LanguageServer],
    renames: &[(PathBuf, PathBuf)],
    served: impl Fn(&(PathBuf, PathBuf)) -> &PathBuf,
) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    for rename in renames {
        for client in clients_for(editor, servers, served(rename)) {
            let files = (uri::from_path(&rename.0), uri::from_path(&rename.1));
            match parts
                .iter_mut()
                .find(|part| Rc::ptr_eq(&part.client, &client))
            {
                Some(part) => part.files.push(files),
                None => parts.push(Part {
                    client,
                    files: vec![files],
                    ask_again: false,
                }),
            }
        }
    }
    parts
}

/// The servers running for a directory's projects, or those for a file, started if
/// needed.
pub(crate) fn clients_for(
    editor: &mut Editor,
    servers: &'static [LanguageServer],
    path: &Path,
) -> Vec<Rc<Client>> {
    if path.is_dir() {
        return editor
            .plugin_state::<Clients>()
            .running()
            .filter(|(root, _)| path.starts_with(root))
            .map(|(_, client)| Rc::clone(client))
            .collect();
    }
    let mode = editor.mode_for(Some(path));
    let notifier = editor.notifier();
    let clients = editor.plugin_state::<Clients>();
    servers
        .iter()
        .filter(|server| server.language_id(mode).is_some())
        .filter_map(|server| {
            clients
                .client_for(server, path, Arc::clone(&notifier))
                .ok()
                .flatten()
        })
        .collect()
}

fn collect_answers(editor: &mut Editor) {
    let clients: Vec<Rc<Client>> = editor
        .plugin_state::<Clients>()
        .running()
        .map(|(_, client)| Rc::clone(client))
        .collect();
    for client in clients {
        for renamed in client.take_renamed() {
            let asked = &mut editor.plugin_state::<Asked>().by_key;
            let Some(collected) = asked.get_mut(&renamed.id) else {
                continue;
            };
            let Some(index) = collected
                .unanswered
                .iter()
                .position(|part| Rc::ptr_eq(&part.client, &client))
            else {
                continue;
            };
            if !renamed.settled {
                collected.unanswered[index].ask_again = true;
                continue;
            }
            collected.unanswered.remove(index);
            let done = collected.unanswered.is_empty();
            let edits = match renamed.edit {
                Some(edit) => file_edits(editor, edit, renamed.encoding).unwrap_or_else(|error| {
                    editor.set_message(format!("References cannot be updated: {error}"));
                    Vec::new()
                }),
                None => Vec::new(),
            };
            let asked = &mut editor.plugin_state::<Asked>().by_key;
            if let Some(collected) = asked.get_mut(&renamed.id) {
                collected.edits.extend(edits);
            }
            if done && let Some(collected) = asked.remove(&renamed.id) {
                answer(editor, collected.asker, collected.edits);
            }
        }
    }
}

fn ask_settled_servers_again(editor: &mut Editor) {
    for (key, collected) in &mut editor.plugin_state::<Asked>().by_key {
        for part in &mut collected.unanswered {
            if part.ask_again && part.client.is_settled() {
                part.ask_again = false;
                part.client.send(Outgoing::WillRename {
                    id: *key,
                    files: part.files.clone(),
                });
            }
        }
    }
}
