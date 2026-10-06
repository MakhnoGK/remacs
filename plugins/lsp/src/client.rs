use std::cell::Cell;
use std::collections::HashMap;
use std::io::{self, BufReader, BufWriter};
use std::iter;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
    Initialized, Notification, PublishDiagnostics,
};
use lsp_types::request::{
    ApplyWorkspaceEdit, CodeActionRequest, CodeActionResolveRequest, Completion,
    DocumentDiagnosticRequest, ExecuteCommand, Formatting, GotoDefinition, HoverRequest,
    Initialize, RangeFormatting, Request, ResolveCompletionItem, WillRenameFiles,
    WorkspaceConfiguration, WorkspaceDiagnosticRefresh, WorkspaceSymbolRequest,
};
use lsp_types::{
    ApplyWorkspaceEditParams, CodeAction, CodeActionOrCommand, CompletionItem,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, InitializedParams, Position, PublishDiagnosticsParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem, TextEdit, Uri,
    VersionedTextDocumentIdentifier, WorkspaceEdit,
};
use remacs_actions::Action;
use remacs_completion::{Item, List, Trigger};
use remacs_core::Notifier;
use remacs_format::{Failure, Formatted};
use remacs_text::{Edit, Snapshot};
use serde_json::{Value, json};

use crate::LanguageServer;
use crate::completion::{self, CompletionSupport, Origin};
use crate::markdown::CodeModes;
use crate::position::{Encoding, from_lsp, to_lsp};
use crate::settings::ServerSettings;
use crate::symbols::{self, FoundSymbol};
use crate::transport::{read_message, write_message};
use crate::{code_action, definition, hover, uri};

const INITIALIZE_ID: u64 = 0;
/// rust-analyzer's, with `quiescent` false while it loads the project.
const SERVER_STATUS: &str = "experimental/serverStatus";

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
        modes: CodeModes,
    },
    /// Asks for the rest of item `index` of list `id`, such as its documentation.
    Resolve {
        id: u64,
        index: usize,
        item: Box<CompletionItem>,
        text: Snapshot,
        start: usize,
        modes: CodeModes,
    },
    /// Asks about the symbol at `offset` of `text`; `id` is the asking plugin's.
    AtPoint {
        question: Question,
        uri: Uri,
        id: u64,
        text: Snapshot,
        offset: usize,
        modes: CodeModes,
    },
    /// Asks for the actions for `range` of `text`, passing back the problems reported
    /// there; `id` is the actions plugin's.
    CodeActions {
        uri: Uri,
        id: u64,
        text: Snapshot,
        range: Range<usize>,
        /// As the server reported them for `text`.
        diagnostics: Vec<lsp_types::Diagnostic>,
    },
    /// Asks for what an action left out, such as its edit.
    ResolveAction {
        id: u64,
        action: Box<CodeAction>,
    },
    /// Runs an action's command; the server may then ask to apply edits.
    Execute {
        id: u64,
        command: lsp_types::Command,
    },
    /// Asks for the edits that format `range` of `text`, or all of it; `id` is the format
    /// plugin's.
    Format {
        uri: Uri,
        id: u64,
        text: Snapshot,
        range: Option<Range<usize>>,
    },
    /// Asks what renaming files, from and to, would change in others; `id` is dired's.
    WillRename {
        id: u64,
        files: Vec<(Uri, Uri)>,
    },
    /// Asks for the project's symbols matching `query`; `id` is xref's.
    Symbols {
        id: u64,
        query: String,
    },
    /// Answers a request the server made.
    Reply {
        id: Value,
        result: Value,
    },
}

/// The edits a server says renaming files calls for; none when it cannot tell.
pub(crate) struct Renamed {
    pub id: u64,
    pub edit: Option<WorkspaceEdit>,
    pub encoding: Encoding,
    /// Given while the server was still loading the project, so it may have missed files.
    pub settled: bool,
}

/// The symbols a server found for a query; none when it cannot look.
pub(crate) struct Found {
    pub id: u64,
    pub symbols: Vec<FoundSymbol>,
    pub encoding: Encoding,
    /// Given while the server was still loading the project, so it may have missed some.
    pub settled: bool,
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
    CodeActions {
        id: u64,
        menu: Vec<Action>,
        /// As the server sent them, to carry one out later.
        actions: Vec<CodeActionOrCommand>,
        encoding: Encoding,
    },
    ResolvedAction {
        id: u64,
        action: CodeAction,
        encoding: Encoding,
        /// Given while the server was still loading the project; rust-analyzer then
        /// answers with the action as it was sent.
        settled: bool,
    },
    ActionFailed {
        id: u64,
        message: String,
    },
    Formatted {
        id: u64,
        formatted: Formatted,
    },
    /// The server asks for edits, e.g. from an action's command; `reply` is the request's
    /// id, to answer whether they were made.
    ApplyEdit {
        reply: Value,
        edit: WorkspaceEdit,
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
        origin: Origin,
    },
    CodeActions {
        id: u64,
    },
    ResolveAction {
        id: u64,
    },
    Execute {
        id: u64,
    },
    Format {
        id: u64,
        text: Snapshot,
    },
    WillRename {
        id: u64,
    },
    Symbols {
        id: u64,
    },
}

impl Pending {
    /// Whether a failure is answered too, so the user hears of it; other failed
    /// requests, e.g. cancelled by a newer edit, are dropped.
    fn answers_failure(&self) -> bool {
        !matches!(
            self,
            Pending::Pull { .. } | Pending::Complete { .. } | Pending::Resolve { .. }
        )
    }
}

/// What the reader thread collects for the editor.
#[derive(Default)]
struct Inbox {
    published: HashMap<PathBuf, Published>,
    requests: HashMap<u64, Pending>,
    answers: Vec<Answer>,
    /// Kept apart from `answers`, which are for the window buffer's document.
    renamed: Vec<Renamed>,
    found: Vec<Found>,
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
    will_rename: bool,
    formatting: bool,
    range_formatting: bool,
    workspace_symbols: bool,
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
    /// The action whose command ran last, which edits the server asks for belong to.
    executing: Cell<Option<u64>>,
    /// False while the server says it is busy loading the project; servers that never say
    /// are taken as settled.
    settled: Arc<AtomicBool>,
}

impl Client {
    /// `settings` change the built-in `server`, as `lsp.toml` says.
    pub fn start(
        server: &LanguageServer,
        settings: Option<&ServerSettings>,
        root: &Path,
        notifier: Notifier,
    ) -> io::Result<Self> {
        let program = settings
            .and_then(ServerSettings::command)
            .unwrap_or_else(|| program(server, root));
        let mut command = Command::new(program);
        match settings.and_then(ServerSettings::args) {
            Some(args) => command.args(args),
            None => command.args(server.args),
        };
        let mut child = command
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
        let settled = Arc::new(AtomicBool::new(true));
        let writer = Writer {
            server: server.name,
            stdin: BufWriter::new(stdin),
            inbox: Arc::clone(&inbox),
            notifier: Arc::clone(&notifier),
            next_id: INITIALIZE_ID + 1,
        };
        let options = settings
            .and_then(ServerSettings::initialization_options)
            .cloned()
            .or_else(|| server.initialization_options.map(built_in_json));
        let initialize = initialize_params(options, root);
        thread::Builder::new()
            .name(format!("{} writer", server.name))
            .spawn(move || writer.write_all(initialize, negotiated, queued))?;
        let reader = Reader {
            server: server.name,
            configuration: settings
                .and_then(ServerSettings::configuration)
                .cloned()
                .or_else(|| server.configuration.map(built_in_json))
                .unwrap_or_default(),
            replies: outgoing.clone(),
            agreed: Some(agreed),
            encoding: Encoding::Utf16,
            inbox: Arc::clone(&inbox),
            pull_generation: Arc::clone(&pull_generation),
            notifier: Arc::clone(&notifier),
            settled: Arc::clone(&settled),
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
            executing: Cell::new(None),
            settled,
        })
    }

    pub fn is_settled(&self) -> bool {
        self.settled.load(Ordering::Relaxed)
    }

    /// Runs the command of action `id` of the actions plugin.
    pub fn execute(&self, id: u64, command: lsp_types::Command) {
        self.executing.set(Some(id));
        self.send(Outgoing::Execute { id, command });
    }

    pub fn executing(&self) -> Option<u64> {
        self.executing.get()
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

    pub fn take_renamed(&self) -> Vec<Renamed> {
        std::mem::take(&mut lock(&self.inbox).renamed)
    }

    pub fn take_found(&self) -> Vec<Found> {
        std::mem::take(&mut lock(&self.inbox).found)
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

/// The project's own copy in `node_modules/.bin` of the root or above, for servers that
/// prefer it; else the command, found on `PATH`.
fn program(server: &LanguageServer, root: &Path) -> PathBuf {
    server
        .node_modules
        .then(|| {
            root.ancestors()
                .map(|directory| directory.join("node_modules/.bin").join(server.command))
                .find(|program| program.is_file())
        })
        .flatten()
        .unwrap_or_else(|| server.command.into())
}

fn built_in_json(json: &str) -> Value {
    serde_json::from_str(json).expect("built-in server settings are JSON")
}

fn initialize_params(options: Option<Value>, root: &Path) -> Value {
    let root_uri = uri::from_path(root);
    let name = root.file_name().map(|name| name.to_string_lossy());
    json!({
        "processId": std::process::id(),
        "clientInfo": { "name": "remacs" },
        "rootUri": root_uri,
        "workspaceFolders": [{ "uri": root_uri, "name": name }],
        "initializationOptions": options,
        "capabilities": {
            "general": { "positionEncodings": ["utf-8", "utf-16"] },
            // rust-analyzer then says when it is done loading the project.
            "experimental": { "serverStatusNotification": true },
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
                // Without literal support servers send bare commands only.
                "codeAction": {
                    "codeActionLiteralSupport": {
                        "codeActionKind": {
                            "valueSet": [
                                "", "quickfix", "refactor", "refactor.extract", "refactor.inline",
                                "refactor.rewrite", "source", "source.organizeImports",
                            ],
                        },
                    },
                    "isPreferredSupport": true,
                    "dataSupport": true,
                    "resolveSupport": { "properties": ["edit"] },
                },
            },
            "workspace": {
                "applyEdit": true,
                "workspaceEdit": { "documentChanges": true },
                "fileOperations": { "willRename": true },
                // Every kind, up to type parameters; else servers keep to the first 18.
                "symbol": { "symbolKind": { "valueSet": (1..=26).collect::<Vec<u8>>() } },
            },
        },
    })
}

struct Writer {
    server: &'static str,
    stdin: BufWriter<ChildStdin>,
    inbox: SharedInbox,
    notifier: Notifier,
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
        let origin = |text: &Snapshot, start: usize, modes: CodeModes| Origin {
            text: text.clone(),
            start,
            encoding: negotiated.encoding,
            modes,
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
                modes,
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
                    origin: origin(text, *start, *modes),
                };
                self.ask(Completion::METHOD, params, pending)
            }
            Outgoing::Resolve {
                id,
                index,
                item,
                text,
                start,
                modes,
            } => {
                if !completion.is_some_and(CompletionSupport::resolves) {
                    return Ok(());
                }
                let pending = Pending::Resolve {
                    id: *id,
                    index: *index,
                    origin: origin(text, *start, *modes),
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
                modes,
            } => {
                let params = json!({
                    "textDocument": TextDocumentIdentifier::new(uri.clone()),
                    "position": to_lsp(text, *offset, negotiated.encoding),
                });
                let pending = Pending::AtPoint {
                    question: *question,
                    id: *id,
                    origin: origin(text, *offset, *modes),
                };
                self.ask(question.method(), params, pending)
            }
            Outgoing::CodeActions {
                uri,
                id,
                text,
                range,
                diagnostics,
            } => {
                let range = lsp_types::Range::new(
                    to_lsp(text, range.start, negotiated.encoding),
                    to_lsp(text, range.end, negotiated.encoding),
                );
                let diagnostics: Vec<_> = diagnostics
                    .iter()
                    .filter(|diagnostic| {
                        diagnostic.range.start <= range.end && diagnostic.range.end >= range.start
                    })
                    .collect();
                let params = json!({
                    "textDocument": TextDocumentIdentifier::new(uri.clone()),
                    "range": range,
                    "context": { "diagnostics": diagnostics, "triggerKind": 1 },
                });
                self.ask(
                    CodeActionRequest::METHOD,
                    params,
                    Pending::CodeActions { id: *id },
                )
            }
            Outgoing::ResolveAction { id, action } => {
                let params = serde_json::to_value(action)?;
                let pending = Pending::ResolveAction { id: *id };
                self.ask(CodeActionResolveRequest::METHOD, params, pending)
            }
            Outgoing::Execute { id, command } => {
                let mut params = json!({ "command": command.command });
                if let Some(arguments) = &command.arguments {
                    params["arguments"] = json!(arguments);
                }
                self.ask(ExecuteCommand::METHOD, params, Pending::Execute { id: *id })
            }
            Outgoing::WillRename { id, files } => {
                if !negotiated.will_rename {
                    lock(&self.inbox).renamed.push(Renamed {
                        id: *id,
                        edit: None,
                        encoding: negotiated.encoding,
                        settled: true,
                    });
                    (self.notifier)();
                    return Ok(());
                }
                let files: Vec<_> = files
                    .iter()
                    .map(|(old, new)| json!({ "oldUri": old, "newUri": new }))
                    .collect();
                let params = json!({ "files": files });
                let pending = Pending::WillRename { id: *id };
                self.ask(WillRenameFiles::METHOD, params, pending)
            }
            Outgoing::Symbols { id, query } => {
                if !negotiated.workspace_symbols {
                    lock(&self.inbox).found.push(Found {
                        id: *id,
                        symbols: Vec::new(),
                        encoding: negotiated.encoding,
                        settled: true,
                    });
                    (self.notifier)();
                    return Ok(());
                }
                let params = json!({ "query": query });
                let pending = Pending::Symbols { id: *id };
                self.ask(WorkspaceSymbolRequest::METHOD, params, pending)
            }
            Outgoing::Format {
                uri,
                id,
                text,
                range,
            } => {
                let supported = match range {
                    Some(_) => negotiated.range_formatting,
                    None => negotiated.formatting,
                };
                if !supported {
                    let what = if range.is_some() { "a region" } else { "files" };
                    let message = format!("{} does not format {what}", self.server);
                    lock(&self.inbox).answers.push(Answer::Formatted {
                        id: *id,
                        formatted: Err(Failure::Unsupported(message)),
                    });
                    (self.notifier)();
                    return Ok(());
                }
                // Formatters follow the project's own settings, such as rustfmt.toml.
                let mut params = json!({
                    "textDocument": TextDocumentIdentifier::new(uri.clone()),
                    "options": { "tabSize": 4, "insertSpaces": true, "insertFinalNewline": true },
                });
                let method = match range {
                    Some(range) => {
                        params["range"] = json!(lsp_types::Range::new(
                            to_lsp(text, range.start, negotiated.encoding),
                            to_lsp(text, range.end, negotiated.encoding),
                        ));
                        RangeFormatting::METHOD
                    }
                    None => Formatting::METHOD,
                };
                let pending = Pending::Format {
                    id: *id,
                    text: text.clone(),
                };
                self.ask(method, params, pending)
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
        | (Outgoing::Complete { uri, .. }, Outgoing::Complete { uri: other, .. })
        | (Outgoing::CodeActions { uri, .. }, Outgoing::CodeActions { uri: other, .. }) => {
            uri == other
        }
        (
            Outgoing::AtPoint { question, .. },
            Outgoing::AtPoint {
                question: other, ..
            },
        ) => question == other,
        (Outgoing::Symbols { .. }, Outgoing::Symbols { .. }) => true,
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
            | Outgoing::AtPoint { .. }
            | Outgoing::CodeActions { .. }
            | Outgoing::ResolveAction { .. }
            | Outgoing::Execute { .. }
            | Outgoing::Format { .. }
            | Outgoing::WillRename { .. }
            | Outgoing::Symbols { .. } => unreachable!("the writer numbers requests"),
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

/// A capability given as `true` or as its options.
fn is_provided(capability: &Value) -> bool {
    capability.as_bool().unwrap_or(capability.is_object())
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
    /// Answered for every `workspace/configuration` item; `null` for the defaults.
    configuration: Value,
    replies: Sender<Outgoing>,
    /// Taken once `initialize` is answered.
    agreed: Option<Sender<Negotiated>>,
    encoding: Encoding,
    inbox: SharedInbox,
    pull_generation: Arc<AtomicU64>,
    notifier: Notifier,
    settled: Arc<AtomicBool>,
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
            (Some(SERVER_STATUS), None) => {
                let settled = params["quiescent"].as_bool().unwrap_or(true);
                if self.settled.swap(settled, Ordering::Relaxed) != settled {
                    (self.notifier)();
                }
            }
            (None, Some(id)) => match id.as_u64() {
                Some(INITIALIZE_ID) => self.initialized(&message),
                Some(id) => self.answered(id, message),
                None => {}
            },
            _ => {}
        }
    }

    /// Edits the server asks for are answered by the editor once it made them.
    /// Configuration is the server's own, else its defaults; everything else gets an
    /// empty answer.
    fn answer(&mut self, method: &str, params: &Value, id: Value) {
        let result = match method {
            ApplyWorkspaceEdit::METHOD => {
                match serde_json::from_value::<ApplyWorkspaceEditParams>(params.clone()) {
                    Ok(params) => {
                        let encoding = self.encoding;
                        return self.deliver(Answer::ApplyEdit {
                            reply: id,
                            edit: params.edit,
                            encoding,
                        });
                    }
                    Err(error) => json!({ "applied": false, "failureReason": error.to_string() }),
                }
            }
            WorkspaceConfiguration::METHOD => {
                let items = params["items"].as_array().map_or(0, Vec::len);
                Value::Array(vec![self.configuration.clone(); items])
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
            will_rename: capabilities["workspace"]["fileOperations"]["willRename"].is_object(),
            formatting: is_provided(&capabilities["documentFormattingProvider"]),
            range_formatting: is_provided(&capabilities["documentRangeFormattingProvider"]),
            workspace_symbols: is_provided(&capabilities["workspaceSymbolProvider"]),
        });
    }

    /// A failed question is answered with nothing, so the user hears that there is
    /// nothing to show; a failed action with why.
    fn answered(&mut self, id: u64, mut response: Value) {
        let Some(pending) = lock(&self.inbox).requests.remove(&id) else {
            return;
        };
        let error = response.get("error").map(|error| {
            error["message"]
                .as_str()
                .unwrap_or("no reason given")
                .to_owned()
        });
        if error.is_some() && !pending.answers_failure() {
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
                origin,
            } => {
                let response = serde_json::from_value(result).unwrap_or_default();
                let answer = hover::answer(response, &origin.text, origin.encoding, origin.modes);
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
            Pending::CodeActions { id } => {
                let mut actions: Vec<CodeActionOrCommand> =
                    serde_json::from_value(result).unwrap_or_default();
                actions.retain(|action| {
                    !matches!(action, CodeActionOrCommand::CodeAction(action) if action.disabled.is_some())
                });
                self.deliver(Answer::CodeActions {
                    id,
                    menu: code_action::menu(&actions),
                    actions,
                    encoding: self.encoding,
                });
            }
            Pending::ResolveAction { id } => {
                let answer = match (error, serde_json::from_value(result)) {
                    (None, Ok(action)) => Answer::ResolvedAction {
                        id,
                        action,
                        encoding: self.encoding,
                        settled: self.settled.load(Ordering::Relaxed),
                    },
                    (error, _) => Answer::ActionFailed {
                        id,
                        message: error.unwrap_or_else(|| "the server sent no action".into()),
                    },
                };
                self.deliver(answer);
            }
            Pending::Execute { id } => {
                if let Some(message) = error {
                    self.deliver(Answer::ActionFailed { id, message });
                }
            }
            Pending::Format { id, text } => {
                let formatted = match error {
                    Some(message) => Err(Failure::Failed(format!("{}: {message}", self.server))),
                    None => {
                        let edits: Vec<TextEdit> =
                            serde_json::from_value(result).unwrap_or_default();
                        Ok(edits
                            .into_iter()
                            .map(|edit| {
                                let start = from_lsp(&text, edit.range.start, self.encoding);
                                let end = from_lsp(&text, edit.range.end, self.encoding);
                                (start..end.max(start), edit.new_text)
                            })
                            .collect())
                    }
                };
                self.deliver(Answer::Formatted { id, formatted });
            }
            Pending::WillRename { id } => {
                let edit = error
                    .is_none()
                    .then(|| serde_json::from_value(result).ok())
                    .flatten();
                lock(&self.inbox).renamed.push(Renamed {
                    id,
                    edit,
                    encoding: self.encoding,
                    settled: self.settled.load(Ordering::Relaxed),
                });
                (self.notifier)();
            }
            Pending::Symbols { id } => {
                let response = error
                    .is_none()
                    .then(|| serde_json::from_value(result).ok())
                    .flatten();
                lock(&self.inbox).found.push(Found {
                    id,
                    symbols: symbols::found(response),
                    encoding: self.encoding,
                    settled: self.settled.load(Ordering::Relaxed),
                });
                (self.notifier)();
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
