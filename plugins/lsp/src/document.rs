use std::collections::{HashMap, HashSet, VecDeque};
use std::mem;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use lsp_types::{CompletionItem, DiagnosticSeverity, NumberOrString, Uri};
use remacs_completion::Request;
use remacs_core::{Editor, Notifier};
use remacs_diagnostics::{self as diagnostics, Diagnostic, Severity};
use remacs_text::Snapshot;

use crate::LanguageServer;
use crate::client::{Answer, Client, Outgoing, Published, Question};
use crate::completion::Origin;
use crate::markdown::CodeModes;
use crate::position::{Encoding, from_lsp};
use crate::{definition, uri};

/// Servers hold whole files in memory and reparse them; larger ones stay unsynced.
const MAX_SYNCED_BYTES: usize = 16 << 20;
/// Sent texts kept for diagnostics that may still arrive for them.
const MAX_PENDING_VERSIONS: usize = 32;

/// A buffer's file as its server knows it, like an eglot-managed buffer. Dropping it,
/// e.g. with the buffer, closes the file on the server.
struct Document {
    client: Rc<Client>,
    path: PathBuf,
    uri: Uri,
    language: &'static str,
    /// What code in its documentation is highlighted as.
    modes: CodeModes,
    version: i32,
    modified: bool,
    /// Versions sent and not yet answered with diagnostics, oldest first; the last one is
    /// what the server has now.
    pending: VecDeque<(i32, Snapshot)>,
    /// The client's pull generation when the diagnostics were last pulled.
    pulled: Option<u64>,
    /// The newest completion list, as the server sent it, for resolving its items.
    listed: Option<Listed>,
}

struct Listed {
    id: u64,
    items: Vec<CompletionItem>,
    origin: Origin,
}

impl Document {
    fn open(
        client: Rc<Client>,
        path: PathBuf,
        language: &'static str,
        modes: CodeModes,
        text: Snapshot,
        modified: bool,
    ) -> Self {
        let uri = uri::from_path(&path);
        client.send(Outgoing::Open {
            uri: uri.clone(),
            language,
            version: 0,
            text: text.clone(),
        });
        Self {
            client,
            path,
            uri,
            language,
            modes,
            version: 0,
            modified,
            pending: VecDeque::from([(0, text)]),
            pulled: None,
            listed: None,
        }
    }

    /// The text the server has now.
    fn sent_text(&self) -> Snapshot {
        self.pending
            .back()
            .expect("the sent text is pending")
            .1
            .clone()
    }

    /// Sends what changed since the last call; a save shows as the buffer turning
    /// unmodified. Returns the diagnostics published since, with the text they refer to.
    fn sync(&mut self, text: Snapshot, modified: bool) -> Option<(Snapshot, Vec<Diagnostic>)> {
        let sent = self.sent_text();
        let edit = text.edit_from(&sent);
        if let Some(edit) = edit {
            self.version += 1;
            self.client.send(Outgoing::Change {
                uri: self.uri.clone(),
                version: self.version,
                old: sent,
                new: text.clone(),
                edit,
            });
            self.pending.push_back((self.version, text));
            if self.pending.len() > MAX_PENDING_VERSIONS {
                self.pending.pop_front();
            }
        }
        if self.modified && !modified {
            self.client.send(Outgoing::Save {
                uri: self.uri.clone(),
            });
            if self.client.pulls_diagnostics() {
                self.client.pull_all_again();
            }
        }
        self.modified = modified;
        self.pull(edit.is_some());
        self.take_diagnostics()
    }

    /// Asks a server that does not publish on its own, after a change or when all files
    /// are due again.
    fn pull(&mut self, changed: bool) {
        let generation = self.client.pull_generation();
        if !self.client.pulls_diagnostics() || !changed && self.pulled == Some(generation) {
            return;
        }
        self.pulled = Some(generation);
        self.client.send(Outgoing::Pull {
            uri: self.uri.clone(),
            path: self.path.clone(),
            version: self.version,
        });
    }

    /// Diagnostics for a version no longer pending are stale and dropped.
    fn take_diagnostics(&mut self) -> Option<(Snapshot, Vec<Diagnostic>)> {
        let Published {
            version,
            diagnostics,
            encoding,
        } = self.client.take_published(&self.path)?;
        let index = match version {
            Some(version) => self.pending.iter().position(|(sent, _)| *sent == version)?,
            None => self.pending.len() - 1,
        };
        self.pending.drain(..index);
        let text = self.pending[0].1.clone();
        let diagnostics = diagnostics
            .into_iter()
            .filter_map(|diagnostic| convert(diagnostic, &text, encoding))
            .collect();
        Some((text, diagnostics))
    }

    fn ask(&self, question: Question, id: u64, offset: usize) {
        self.client.send(Outgoing::AtPoint {
            question,
            uri: self.uri.clone(),
            id,
            text: self.sent_text(),
            offset,
            modes: self.modes,
        });
    }

    fn request(&self, request: Request) {
        let message = match request {
            Request::Complete {
                id,
                start,
                offset,
                trigger,
            } => Outgoing::Complete {
                uri: self.uri.clone(),
                id,
                text: self.sent_text(),
                start,
                offset,
                trigger,
                modes: self.modes,
            },
            Request::Resolve { id, index } => {
                let listed = self.listed.as_ref().filter(|listed| listed.id == id);
                let Some((listed, item)) =
                    listed.and_then(|listed| Some((listed, listed.items.get(index)?)))
                else {
                    return;
                };
                Outgoing::Resolve {
                    id,
                    index,
                    item: Box::new(item.clone()),
                    text: listed.origin.text.clone(),
                    start: listed.origin.start,
                    modes: listed.origin.modes,
                }
            }
        };
        self.client.send(message);
    }

    /// Answers since the last call; the newest completion list is kept for resolving.
    fn take_answers(&mut self) -> Vec<Answer> {
        let mut answers = self.client.take_answers();
        for answer in &mut answers {
            if let Answer::Listed {
                id, items, origin, ..
            } = answer
            {
                self.listed = Some(Listed {
                    id: *id,
                    items: mem::take(items),
                    origin: origin.clone(),
                });
            }
        }
        answers
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        self.client.send(Outgoing::Close {
            uri: self.uri.clone(),
        });
        self.client.take_published(&self.path);
    }
}

/// Servers started so far, one per server and project root.
#[derive(Default)]
struct Clients {
    running: HashMap<(&'static str, PathBuf), Rc<Client>>,
    /// Servers that could not start; they are not tried again.
    failed: HashSet<&'static str>,
}

impl Clients {
    /// The server for `path`'s project, started on first use; `Err` the one time it fails
    /// to start.
    fn client_for(
        &mut self,
        server: &'static LanguageServer,
        path: &Path,
        notifier: Notifier,
    ) -> Result<Option<Rc<Client>>, String> {
        if self.failed.contains(server.name) {
            return Ok(None);
        }
        let root = project_root(path, server.root_markers);
        if let Some(client) = self.running.get(&(server.name, root.clone())) {
            return Ok(Some(Rc::clone(client)));
        }
        match Client::start(server, &root, notifier) {
            Ok(client) => {
                let client = Rc::new(client);
                self.running.insert((server.name, root), Rc::clone(&client));
                Ok(Some(client))
            }
            Err(error) => {
                self.failed.insert(server.name);
                Err(format!("{}: {error}", server.command))
            }
        }
    }
}

/// Hands answers over, keeps every buffer's server up to date and passes its diagnostics
/// on, then sends what the plugins ask, including what the answers made them ask. Runs
/// after every command and whenever a server published or answered something.
pub(crate) fn sync(editor: &mut Editor, servers: &'static [LanguageServer]) {
    hand_over_answers(editor);
    // After the answers, so that a file a definition opened is attached at once.
    for index in 0..editor.buffers().len() {
        attach(editor, index, servers);
        let buffer = &mut editor.buffers_mut()[index];
        let (text, modified) = (buffer.text().snapshot(), buffer.is_modified());
        let reported = buffer
            .local_mut::<Document>()
            .and_then(|document| document.sync(text, modified));
        if let Some((text, diagnostics)) = reported {
            diagnostics::publish(buffer, text, diagnostics);
        }
    }
    send_requests(editor);
}

fn hand_over_answers(editor: &mut Editor) {
    let answers = editor
        .window_buffer_mut()
        .local_mut::<Document>()
        .map(Document::take_answers)
        .unwrap_or_default();
    for answer in answers {
        match answer {
            Answer::Listed { id, list, .. } => remacs_completion::deliver(editor, id, list),
            Answer::Resolved { id, index, item } => {
                remacs_completion::resolved(editor, id, index, item);
            }
            Answer::Hover { id, answer } => remacs_documentation::deliver(editor, id, answer),
            Answer::Definitions {
                id,
                targets,
                encoding,
            } => {
                let locations = definition::locations(editor, targets, encoding);
                remacs_xref::deliver(editor, id, locations);
            }
        }
    }
}

/// Sends the window buffer's requests to its server; without one, questions are answered
/// with nothing at once.
fn send_requests(editor: &mut Editor) {
    let completions = remacs_completion::take_requests(editor);
    let hovers = remacs_documentation::take_requests(editor);
    let definitions = remacs_xref::take_requests(editor);
    let Some(document) = editor.window_buffer().local::<Document>() else {
        for request in hovers {
            remacs_documentation::deliver(editor, request.id, None);
        }
        for request in definitions {
            remacs_xref::deliver(editor, request.id, Vec::new());
        }
        return;
    };
    for request in completions {
        document.request(request);
    }
    for request in hovers {
        document.ask(Question::Hover, request.id, request.offset);
    }
    for request in definitions {
        document.ask(Question::Definition, request.id, request.offset);
    }
}

/// Opens the buffer's file on the server for its mode; closes it first when the file or
/// the mode changed.
fn attach(editor: &mut Editor, index: usize, servers: &'static [LanguageServer]) {
    let buffer = &editor.buffers()[index];
    let wanted = buffer
        .path()
        .filter(|_| buffer.text().len() <= MAX_SYNCED_BYTES)
        .and_then(|path| {
            servers.iter().find_map(|server| {
                let language = server.language_id(buffer.mode())?;
                Some((server, language, path.to_path_buf()))
            })
        });
    let current = buffer
        .local::<Document>()
        .map(|document| (document.path.as_path(), document.language));
    if wanted
        .as_ref()
        .map(|(_, language, path)| (path.as_path(), *language))
        == current
    {
        return;
    }
    let buffer = &mut editor.buffers_mut()[index];
    if buffer.local::<Document>().is_some() {
        buffer.remove_local::<Document>();
        diagnostics::clear(buffer);
    }
    let Some((server, language, path)) = wanted else {
        return;
    };
    let notifier = editor.notifier();
    let client = match editor
        .plugin_state::<Clients>()
        .client_for(server, &path, notifier)
    {
        Ok(Some(client)) => client,
        Ok(None) => return,
        Err(message) => return editor.set_message(message),
    };
    let buffer = &mut editor.buffers_mut()[index];
    let text = buffer.text().snapshot();
    let modified = buffer.is_modified();
    let modes = CodeModes {
        document: buffer.mode(),
        languages: server.languages,
    };
    buffer.set_local(Document::open(
        client,
        path,
        language,
        modes,
        text.clone(),
        modified,
    ));
    diagnostics::publish(buffer, text, Vec::new());
}

/// The outermost directory with a root marker inside the Git work tree; else the work
/// tree, else the file's directory. One server then covers a whole Cargo workspace or
/// monorepo.
fn project_root(path: &Path, markers: &[&str]) -> PathBuf {
    let directory = path.parent().unwrap_or(path);
    let mut outermost = None;
    for ancestor in directory.ancestors() {
        if markers.iter().any(|marker| ancestor.join(marker).exists()) {
            outermost = Some(ancestor);
        }
        if ancestor.join(".git").exists() {
            return outermost.unwrap_or(ancestor).to_path_buf();
        }
    }
    outermost.unwrap_or(directory).to_path_buf()
}

/// Hints are left out: servers use them for faded code and to repeat other diagnostics'
/// related locations.
fn convert(
    diagnostic: lsp_types::Diagnostic,
    text: &Snapshot,
    encoding: Encoding,
) -> Option<Diagnostic> {
    let severity = match diagnostic.severity {
        Some(DiagnosticSeverity::HINT) => return None,
        Some(DiagnosticSeverity::INFORMATION) => Severity::Info,
        Some(DiagnosticSeverity::WARNING) => Severity::Warning,
        _ => Severity::Error,
    };
    let start = from_lsp(text, diagnostic.range.start, encoding);
    let end = from_lsp(text, diagnostic.range.end, encoding).max(start);
    Some(Diagnostic {
        range: start..end,
        severity,
        message: diagnostic.message,
        source: source_label(diagnostic.source, diagnostic.code),
    })
}

/// `rustc · E0599`, `clippy · len_without_is_empty`.
fn source_label(source: Option<String>, code: Option<NumberOrString>) -> String {
    let source = source.unwrap_or_default();
    let code = match code {
        Some(NumberOrString::Number(number)) => number.to_string(),
        Some(NumberOrString::String(code)) => code,
        None => String::new(),
    };
    let code = code.strip_prefix(&format!("{source}::")).unwrap_or(&code);
    [source.as_str(), code]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_labels_join_source_and_code() {
        let label = |source: Option<&str>, code: Option<NumberOrString>| {
            source_label(source.map(str::to_owned), code)
        };
        assert_eq!(
            label(Some("rustc"), Some(NumberOrString::String("E0599".into()))),
            "rustc · E0599"
        );
        let lint = NumberOrString::String("clippy::len_without_is_empty".into());
        assert_eq!(
            label(Some("clippy"), Some(lint)),
            "clippy · len_without_is_empty"
        );
        assert_eq!(label(None, Some(NumberOrString::Number(2304))), "2304");
        assert_eq!(label(Some("ts"), None), "ts");
    }
}
