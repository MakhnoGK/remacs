//! Asks servers what renaming files calls for elsewhere, such as the `mod` declarations
//! of renamed Rust modules (`workspace/willRenameFiles`), for names edited in dired.

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use lsp_types::Uri;
use remacs_core::{Editor, FileEdits};
use remacs_dired::RenameRequest;

use crate::LanguageServer;
use crate::client::{Client, Outgoing};
use crate::code_action::file_edits;
use crate::document::Clients;
use crate::uri;

/// Edits collected for each question until every server asked has answered.
#[derive(Default)]
struct Asked {
    by_id: HashMap<u64, Collected>,
}

struct Collected {
    unanswered: Vec<Part>,
    edits: Vec<FileEdits>,
}

/// What one server is asked.
struct Part {
    client: Rc<Client>,
    files: Vec<(Uri, Uri)>,
    /// It answered while still loading the project, so it is asked again once done.
    ask_again: bool,
}

/// Hands answers over and sends dired's new questions.
pub(crate) fn sync(editor: &mut Editor, servers: &'static [LanguageServer]) {
    collect_answers(editor);
    ask_settled_servers_again(editor);
    for request in remacs_dired::take_rename_requests(editor) {
        ask(editor, servers, request);
    }
}

/// Asks each server about the renames of the files it serves, starting it if needed, and
/// of directories in the projects it already serves; with none, answers at once.
fn ask(editor: &mut Editor, servers: &'static [LanguageServer], request: RenameRequest) {
    let mut unanswered: Vec<Part> = Vec::new();
    for (from, to) in &request.renames {
        for client in clients_for(editor, servers, from) {
            let rename = (uri::from_path(from), uri::from_path(to));
            match unanswered
                .iter_mut()
                .find(|part| Rc::ptr_eq(&part.client, &client))
            {
                Some(part) => part.files.push(rename),
                None => unanswered.push(Part {
                    client,
                    files: vec![rename],
                    ask_again: false,
                }),
            }
        }
    }
    if unanswered.is_empty() {
        return remacs_dired::deliver_rename_edits(editor, request.id, Vec::new());
    }
    for part in &unanswered {
        part.client.send(Outgoing::WillRename {
            id: request.id,
            files: part.files.clone(),
        });
    }
    let collected = Collected {
        unanswered,
        edits: Vec::new(),
    };
    editor
        .plugin_state::<Asked>()
        .by_id
        .insert(request.id, collected);
}

fn clients_for(
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
            let asked = &mut editor.plugin_state::<Asked>().by_id;
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
            let asked = &mut editor.plugin_state::<Asked>().by_id;
            if let Some(collected) = asked.get_mut(&renamed.id) {
                collected.edits.extend(edits);
            }
            if done && let Some(collected) = asked.remove(&renamed.id) {
                remacs_dired::deliver_rename_edits(editor, renamed.id, collected.edits);
            }
        }
    }
}

fn ask_settled_servers_again(editor: &mut Editor) {
    for (id, collected) in &mut editor.plugin_state::<Asked>().by_id {
        for part in &mut collected.unanswered {
            if part.ask_again && part.client.is_settled() {
                part.ask_again = false;
                part.client.send(Outgoing::WillRename {
                    id: *id,
                    files: part.files.clone(),
                });
            }
        }
    }
}
