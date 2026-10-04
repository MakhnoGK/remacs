use std::collections::HashMap;
use std::io::{self, BufReader, BufWriter};
use std::iter;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
    Initialized, Notification, PublishDiagnostics,
};
use lsp_types::request::{
    Completion, DocumentDiagnosticRequest, GotoDefinition, HoverRequest, Initialize, Request,
    ResolveCompletionItem, WorkspaceConfiguration, WorkspaceDiagnosticRefresh,
};
use lsp_types::{
    CompletionItem, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, InitializedParams, Position,
    PublishDiagnosticsParams, TextDocumentContentChangeEvent, TextDocumentIdentifier,
    TextDocumentItem, Uri, VersionedTextDocumentIdentifier,
};
use remacs_completion::{Item, List, Trigger};
use remacs_core::Notifier;
use remacs_text::{Edit, Snapshot};
use serde_json::{Value, json};

use crate::LanguageServer;
use crate::completion::{self, CompletionSupport, Origin};
use crate::position::{Encoding, to_lsp};
use crate::transport::{read_message, write_message};
use crate::{definition, hover, uri};

const INITIALIZE_ID: u64 = 0;

/// What the editor tells a server; the writer thread turns it into JSON once it knows how
/// the server counts positions.
pub(crate) enum Outgoing {
    Open {
        uri: Uri,
        language: &'static str,
        version: i32,
        text: Snapshot,
    },
    Change {
        uri: Uri,
        version: i32,
        old: Snapshot,
        new: Snapshot,
        edit: Edit,
    },
    Save {
        uri: Uri,
    },
    Close {
        uri: Uri,
    },
    /// Asks for a file's diagnostics, for servers that only answer when asked.
    Pull {
        uri: Uri,
        path: PathBuf,
        version: i32,
    },
    /// Asks for completions at `offset` of `text`, the text the server has; `id` is the
    /// completion plugin's.
    Complete {
        uri: Uri,
        id: u64,
        text: Snapshot,
        start: usize,
        offset: usize,
        trigger: Trigger,
    },
    /// Asks for the rest of item `index` of list `id`, such as its documentation.
    Resolve {
        id: u64,
        index: usize,
        item: Box<CompletionItem>,
        text: Snapshot,
        start: usize,
    },
    /// Asks about the symbol at `offset` of `text`; `id` is the asking plugin's.
    AtPoint {
        question: Question,
        uri: Uri,
        id: u64,
        text: Snapshot,
        offset: usize,
    },
    /// Answers a request the server made.
    Reply {
        id: Value,
        result: Value,
    },
}

/// The newest diagnostics a server reported for a file.
pub(crate) struct Published {
    pub version: Option<i32>,
    pub diagnostics: Vec<lsp_types::Diagnostic>,
    pub encoding: Encoding,
}

/// What can be asked about the symbol at a position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Question {
    Hover,
    Definition,
}

impl Question {
    fn method(self) -> &'static str {
        match self {
            Question::Hover => HoverRequest::METHOD,
            Question::Definition => GotoDefinition::METHOD,
        }
    }
}

/// What a server answered to the plugins' requests.
pub(crate) enum Answer {
    Listed {
        id: u64,
        list: List,
        /// As the server sent them, to resolve them later.
        items: Vec<CompletionItem>,
        origin: Origin,
    },
    Resolved {
        id: u64,
        index: usize,
        item: Item,
    },
    Hover {
        id: u64,
        answer: Option<remacs_documentation::Answer>,
    },
    /// Positions in the target files, which only the editor has the texts of.
    Definitions {
        id: u64,
        targets: Vec<(PathBuf, Position)>,
        encoding: Encoding,
    },
}

/// A request in flight and what its answer is for.
enum Pending {
    Pull {
        path: PathBuf,
        version: i32,
    },
    Complete {
        id: u64,
        origin: Origin,
    },
    Resolve {
        id: u64,
        index: usize,
        origin: Origin,
    },
    AtPoint {
        question: Question,
        id: u64,
        text: Snapshot,
    },
}

/// What the reader thread collects for the editor.
#[derive(Default)]
struct Inbox {
    published: HashMap<PathBuf, Published>,
    requests: HashMap<u64, Pending>,
    answers: Vec<Answer>,
}

type SharedInbox = Arc<Mutex<Inbox>>;

fn lock(inbox: &SharedInbox) -> MutexGuard<'_, Inbox> {
    inbox.lock().expect("no thread panics holding the inbox")
}

/// How the server wants positions and changes and what it offers, agreed on in
/// `initialize`.
struct Negotiated {
    encoding: Encoding,
    incremental: bool,
    completion: Option<CompletionSupport>,
}

/// A language server process for one project root. A writer thread sends what the editor
/// queues, a reader thread answers the server's requests and collects its diagnostics.
pub(crate) struct Client {
    outgoing: Sender<Outgoing>,
    inbox: SharedInbox,
    pulls_diagnostics: bool,
    /// Bumped whenever every file's diagnostics should be pulled again.
    pull_generation: Arc<AtomicU64>,
    notifier: Notifier,
}

impl Client {
    pub fn start(server: &LanguageServer, root: &Path, notifier: Notifier) -> io::Result<Self> {
        let mut child = Command::new(server.command)
            .args(server.args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let (outgoing, queued) = mpsc::channel();
        let (agreed, negotiated) = mpsc::channel();
        let inbox = SharedInbox::default();
        let pull_generation = Arc::new(AtomicU64::new(0));
        let writer = Writer {
            stdin: BufWriter::new(stdin),
            inbox: Arc::clone(&inbox),
            next_id: INITIALIZE_ID + 1,
        };
        let initialize = initialize_params(server, root);
        thread::Builder::new()
            .name(format!("{} writer", server.name))
            .spawn(move || writer.write_all(initialize, negotiated, queued))?;
        let reader = Reader {
            server: server.name,
            replies: outgoing.clone(),
            agreed: Some(agreed),
            encoding: Encoding::Utf16,
            inbox: Arc::clone(&inbox),
            pull_generation: Arc::clone(&pull_generation),
            notifier: Arc::clone(&notifier),
        };
        thread::Builder::new()
            .name(format!("{} reader", server.name))
            .spawn(move || {
                reader.read_all(stdout);
                let _ = child.wait();
            })?;
        Ok(Self {
            outgoing,
            inbox,
            pulls_diagnostics: server.pull_diagnostics,
            pull_generation,
            notifier,
        })
    }

    pub fn send(&self, message: Outgoing) {
        // Fails only when the server has exited, and then there is no one left to tell.
        let _ = self.outgoing.send(message);
    }

    pub fn take_published(&self, path: &Path) -> Option<Published> {
        lock(&self.inbox).published.remove(path)
    }

    pub fn take_answers(&self) -> Vec<Answer> {
        std::mem::take(&mut lock(&self.inbox).answers)
    }

    pub fn pulls_diagnostics(&self) -> bool {
        self.pulls_diagnostics
    }

    pub fn pull_generation(&self) -> u64 {
        self.pull_generation.load(Ordering::Relaxed)
    }

    /// A save may change what other files report, so all of them are pulled again.
    pub fn pull_all_again(&self) {
        self.pull_generation.fetch_add(1, Ordering::Relaxed);
        (self.notifier)();
    }
}

fn initialize_params(server: &LanguageServer, root: &Path) -> Value {
    let root_uri = uri::from_path(root);
    let name = root.file_name().map(|name| name.to_string_lossy());
    let options = server
        .initialization_options
        .map(|options| serde_json::from_str::<Value>(options).expect("options are JSON"));
    json!({
        "processId": std::process::id(),
        "clientInfo": { "name": "remacs" },
        "rootUri": root_uri,
        "workspaceFolders": [{ "uri": root_uri, "name": name }],
        "initializationOptions": options,
        "capabilities": {
            "general": { "positionEncodings": ["utf-8", "utf-16"] },
            "textDocument": {
                "synchronization": { "didSave": true },
                "publishDiagnostics": { "versionSupport": true },
                // The CSS and HTML servers complete only for clients with snippets.
                "completion": {
                    "completionItem": {
                        "snippetSupport": true,
                        "documentationFormat": ["markdown", "plaintext"],
                        "insertReplaceSupport": true,
                        "labelDetailsSupport": true,
                    },
                    "contextSupport": true,
                },
                "hover": { "contentFormat": ["markdown", "plaintext"] },
                "definition": { "linkSupport": true },
            },
        },
    })
}

struct Writer {
    stdin: BufWriter<ChildStdin>,
    inbox: SharedInbox,
    next_id: u64,
}

impl Writer {
    /// The server must hear nothing but `initialize` until it answers, so the queue waits.
    fn write_all(
        mut self,
        initialize: Value,
        negotiated: Receiver<Negotiated>,
        queued: Receiver<Outgoing>,
    ) -> io::Result<()> {
        write_message(
            &mut self.stdin,
            &request(INITIALIZE_ID, Initialize::METHOD, initialize),
        )?;
        let Ok(negotiated) = negotiated.recv() else {
            return Ok(());
        };
        write_message(
            &mut self.stdin,
            &notification::<Initialized>(InitializedParams {}),
        )?;
        while let Ok(first) = queued.recv() {
            let batch: Vec<_> = iter::once(first).chain(queued.try_iter()).collect();
            for (index, message) in batch.iter().enumerate() {
                if !is_superseded(message, &batch[index + 1..]) {
                    self.write(message, &negotiated)?;
                }
            }
        }
        Ok(())
    }

    fn write(&mut self, message: &Outgoing, negotiated: &Negotiated) -> io::Result<()> {
        let origin = |text: &Snapshot, start: usize| Origin {
            text: text.clone(),
            start,
            encoding: negotiated.encoding,
        };
        let completion = negotiated.completion.as_ref();
        let json = match message {
            Outgoing::Pull { uri, path, version } => {
                let params = json!({ "textDocument": TextDocumentIdentifier::new(uri.clone()) });
                let pending = Pending::Pull {
                    path: path.clone(),
                    version: *version,
                };
                self.ask(DocumentDiagnosticRequest::METHOD, params, pending)
            }
            Outgoing::Complete {
                uri,
                id,
                text,
                start,
                offset,
                trigger,
            } => {
                let Some(context) = completion.and_then(|support| support.context(*trigger)) else {
                    return Ok(());
                };
                let params = json!({
                    "textDocument": TextDocumentIdentifier::new(uri.clone()),
                    "position": to_lsp(text, *offset, negotiated.encoding),
                    "context": context,
                });
                let pending = Pending::Complete {
                    id: *id,
                    origin: origin(text, *start),
                };
                self.ask(Completion::METHOD, params, pending)
            }
            Outgoing::Resolve {
                id,
                index,
                item,
                text,
                start,
            } => {
                if !completion.is_some_and(CompletionSupport::resolves) {
                    return Ok(());
                }
                let pending = Pending::Resolve {
                    id: *id,
                    index: *index,
                    origin: origin(text, *start),
                };
                let params = serde_json::to_value(item)?;
                self.ask(ResolveCompletionItem::METHOD, params, pending)
            }
            Outgoing::AtPoint {
                question,
                uri,
                id,
                text,
                offset,
            } => {
                let params = json!({
                    "textDocument": TextDocumentIdentifier::new(uri.clone()),
                    "position": to_lsp(text, *offset, negotiated.encoding),
                });
                let pending = Pending::AtPoint {
                    question: *question,
                    id: *id,
                    text: text.clone(),
                };
                self.ask(question.method(), params, pending)
            }
            message => message.to_json(negotiated),
        };
        write_message(&mut self.stdin, &json)
    }

    /// Numbers a request and leaves the reader a note of what it is for.
    fn ask(&mut self, method: &str, params: Value, pending: Pending) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        lock(&self.inbox).requests.insert(id, pending);
        request(id, method, params)
    }
}

/// A pull, completion or question that a later one in the same batch repeats is not worth
/// the server's time.
fn is_superseded(message: &Outgoing, later: &[Outgoing]) -> bool {
    later.iter().any(|other| match (message, other) {
        (Outgoing::Pull { uri, .. }, Outgoing::Pull { uri: other, .. })
        | (Outgoing::Complete { uri, .. }, Outgoing::Complete { uri: other, .. }) => uri == other,
        (
            Outgoing::AtPoint { question, .. },
            Outgoing::AtPoint {
                question: other, ..
            },
        ) => question == other,
        _ => false,
    })
}

impl Outgoing {
    fn to_json(&self, negotiated: &Negotiated) -> Value {
        match self {
            Outgoing::Open {
                uri,
                language,
                version,
                text,
            } => notification::<DidOpenTextDocument>(DidOpenTextDocumentParams {
                text_document: TextDocumentItem::new(
                    uri.clone(),
                    (*language).into(),
                    *version,
                    contents(text, 0..text.len()),
                ),
            }),
            Outgoing::Change {
                uri,
                version,
                old,
                new,
                edit,
            } => notification::<DidChangeTextDocument>(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier::new(uri.clone(), *version),
                content_changes: vec![content_change(old, new, *edit, negotiated)],
            }),
            Outgoing::Save { uri } => {
                notification::<DidSaveTextDocument>(DidSaveTextDocumentParams {
                    text_document: TextDocumentIdentifier::new(uri.clone()),
                    text: None,
                })
            }
            Outgoing::Close { uri } => {
                notification::<DidCloseTextDocument>(DidCloseTextDocumentParams {
                    text_document: TextDocumentIdentifier::new(uri.clone()),
                })
            }
            Outgoing::Pull { .. }
            | Outgoing::Complete { .. }
            | Outgoing::Resolve { .. }
            | Outgoing::AtPoint { .. } => unreachable!("the writer numbers requests"),
            Outgoing::Reply { id, result } => {
                json!({ "jsonrpc": "2.0", "id": id, "result": result })
            }
        }
    }
}

fn content_change(
    old: &Snapshot,
    new: &Snapshot,
    edit: Edit,
    negotiated: &Negotiated,
) -> TextDocumentContentChangeEvent {
    if !negotiated.incremental {
        return TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: contents(new, 0..new.len()),
        };
    }
    let start = to_lsp(old, edit.start, negotiated.encoding);
    let end = to_lsp(old, edit.old_end, negotiated.encoding);
    TextDocumentContentChangeEvent {
        range: Some(lsp_types::Range::new(start, end)),
        range_length: None,
        text: contents(new, edit.start..edit.new_end),
    }
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

fn notification<N: Notification>(params: N::Params) -> Value {
    json!({ "jsonrpc": "2.0", "method": N::METHOD, "params": params })
}

fn contents(text: &Snapshot, range: Range<usize>) -> String {
    String::from_utf8_lossy(&text.bytes_in(range)).into_owned()
}

struct Reader {
    server: &'static str,
    replies: Sender<Outgoing>,
    /// Taken once `initialize` is answered.
    agreed: Option<Sender<Negotiated>>,
    encoding: Encoding,
    inbox: SharedInbox,
    pull_generation: Arc<AtomicU64>,
    notifier: Notifier,
}

impl Reader {
    fn read_all(mut self, stdout: ChildStdout) {
        let mut stdout = BufReader::new(stdout);
        loop {
            match read_message(&mut stdout) {
                Ok(Some(message)) => self.handle(message),
                Ok(None) => break,
                Err(error) => {
                    eprintln!("remacs: {}: {error}", self.server);
                    break;
                }
            }
        }
    }

    fn handle(&mut self, mut message: Value) {
        let params = message["params"].take();
        let id = message.get_mut("id").map(Value::take);
        match (message["method"].as_str(), id) {
            (Some(method), Some(id)) => self.answer(method, &params, id),
            (Some(PublishDiagnostics::METHOD), None) => self.publish(params),
            (None, Some(id)) => match id.as_u64() {
                Some(INITIALIZE_ID) => self.initialized(&message),
                Some(id) => self.answered(id, message),
                None => {}
            },
            _ => {}
        }
    }

    /// We take no part in what servers ask of the client: configuration falls back to the
    /// server's defaults and everything else gets an empty answer.
    fn answer(&mut self, method: &str, params: &Value, id: Value) {
        let result = match method {
            WorkspaceConfiguration::METHOD => {
                let items = params["items"].as_array().map_or(0, Vec::len);
                Value::Array(vec![Value::Null; items])
            }
            WorkspaceDiagnosticRefresh::METHOD => {
                self.pull_generation.fetch_add(1, Ordering::Relaxed);
                (self.notifier)();
                Value::Null
            }
            _ => Value::Null,
        };
        let _ = self.replies.send(Outgoing::Reply { id, result });
    }

    fn initialized(&mut self, response: &Value) {
        let Some(agreed) = self.agreed.take() else {
            return;
        };
        if let Some(error) = response.get("error") {
            eprintln!("remacs: {} failed to initialize: {error}", self.server);
            return;
        }
        let capabilities = &response["result"]["capabilities"];
        self.encoding = if capabilities["positionEncoding"] == "utf-8" {
            Encoding::Utf8
        } else {
            Encoding::Utf16
        };
        let sync = &capabilities["textDocumentSync"];
        let full_sync = sync.get("change").unwrap_or(sync).as_u64() == Some(1);
        let _ = agreed.send(Negotiated {
            encoding: self.encoding,
            incremental: !full_sync,
            completion: CompletionSupport::from_capabilities(capabilities),
        });
    }

    /// Failed requests, e.g. cancelled by a newer edit, are dropped; a failed question is
    /// answered with nothing, so the user hears that there is nothing to show.
    fn answered(&mut self, id: u64, mut response: Value) {
        let Some(pending) = lock(&self.inbox).requests.remove(&id) else {
            return;
        };
        if response.get("error").is_some() && !matches!(pending, Pending::AtPoint { .. }) {
            return;
        }
        let result = response["result"].take();
        match pending {
            Pending::Pull { path, version } => self.pulled(path, version, result),
            Pending::Complete { id, origin } => {
                let response = serde_json::from_value(result).unwrap_or_default();
                let (list, items) = completion::list(response, &origin);
                self.deliver(Answer::Listed {
                    id,
                    list,
                    items,
                    origin,
                });
            }
            Pending::Resolve { id, index, origin } => {
                if let Ok(item) = serde_json::from_value(result) {
                    let item = completion::convert(&item, &origin);
                    self.deliver(Answer::Resolved { id, index, item });
                }
            }
            Pending::AtPoint {
                question: Question::Hover,
                id,
                text,
            } => {
                let response = serde_json::from_value(result).unwrap_or_default();
                let answer = hover::answer(response, &text, self.encoding);
                self.deliver(Answer::Hover { id, answer });
            }
            Pending::AtPoint {
                question: Question::Definition,
                id,
                ..
            } => {
                let response = serde_json::from_value(result).unwrap_or_default();
                self.deliver(Answer::Definitions {
                    id,
                    targets: definition::targets(response),
                    encoding: self.encoding,
                });
            }
        }
    }

    fn deliver(&mut self, answer: Answer) {
        lock(&self.inbox).answers.push(answer);
        (self.notifier)();
    }

    fn publish(&mut self, params: Value) {
        let Ok(params) = serde_json::from_value::<PublishDiagnosticsParams>(params) else {
            return;
        };
        if let Some(path) = uri::to_path(&params.uri) {
            self.store(path, params.version, params.diagnostics);
        }
    }

    fn pulled(&mut self, path: PathBuf, version: i32, mut result: Value) {
        if result["kind"] != "full" {
            return;
        }
        if let Ok(diagnostics) = serde_json::from_value(result["items"].take()) {
            self.store(path, Some(version), diagnostics);
        }
    }

    fn store(
        &mut self,
        path: PathBuf,
        version: Option<i32>,
        diagnostics: Vec<lsp_types::Diagnostic>,
    ) {
        let published = Published {
            version,
            diagnostics,
            encoding: self.encoding,
        };
        lock(&self.inbox).published.insert(path, published);
        (self.notifier)();
    }
}
