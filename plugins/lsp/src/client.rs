use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter};
use std::iter;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidRenameFiles,
    DidSaveTextDocument, Initialized, Notification, PublishDiagnostics,
};
use lsp_types::request::{
    ApplyWorkspaceEdit, CodeActionRequest, CodeActionResolveRequest, Completion, DocumentColor,
    DocumentDiagnosticRequest, DocumentSymbolRequest, ExecuteCommand, Formatting, GotoDefinition,
    HoverRequest, Initialize, PrepareRenameRequest, RangeFormatting, References, Rename, Request,
    ResolveCompletionItem, SemanticTokensFullRequest, SemanticTokensRefresh, SignatureHelpRequest,
    WillRenameFiles, WorkspaceConfiguration, WorkspaceDiagnosticRefresh, WorkspaceSymbolRequest,
};
use lsp_types::{
    ApplyWorkspaceEditParams, CodeAction, CodeActionOrCommand, CompletionItem,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, GotoDefinitionResponse, InitializedParams, Position,
    PublishDiagnosticsParams, TextDocumentContentChangeEvent, TextDocumentIdentifier,
    TextDocumentItem, TextEdit, Uri, VersionedTextDocumentIdentifier, WorkspaceEdit,
};
use remacs_actions::Action;
use remacs_completion::{Item, List, Trigger};
use remacs_core::{IndentUnit, Notifier, StatusItem};
use remacs_documentation::{SignatureTrigger, Signatures};
use remacs_format::{Failure, Formatted};
use remacs_text::{Edit, Snapshot};
use serde_json::{Value, json};

use crate::LanguageServer;
use crate::color::{self, Swatch};
use crate::completion::{self, CompletionSupport, Origin};
use crate::markdown::CodeModes;
use crate::position::{Encoding, from_lsp, to_lsp};
use crate::progress::Progress;
use crate::semantic::{self, Legend, Token};
use crate::signature::{self, SignatureSupport};
use crate::symbols::{self, FoundSymbol};
use crate::transport::{read_message, write_message};
use crate::{code_action, definition, document_symbols, hover, symbol_rename, uri};

const INITIALIZE_ID: u64 = 0;
/// rust-analyzer's, with `quiescent` false while it loads the project.
const SERVER_STATUS: &str = "experimental/serverStatus";
/// Vue's server asks for its scripts' TypeScript with `[id, command, args]` and is answered
/// `[id, body]`, each wrapped in one more array as vscode-jsonrpc sends an array argument.
const TSSERVER_REQUEST: &str = "tsserver/request";
const TSSERVER_RESPONSE: &str = "tsserver/response";
const PROGRESS: &str = "$/progress";
/// Stands for the directory of the server's program, as links lead to it, in its
/// initialization options: where plugins installed beside it are found.
const PROGRAM_DIR: &str = "{program-dir}";

/// What a server asks of its project's TypeScript server, in tsserver's own protocol.
pub(crate) struct TsserverRequest {
    pub id: Value,
    command: Value,
    args: Value,
}

/// Where a `tsserver/request` is answered. The asking server waits for every answer, so
/// one dropped unsent, as with a server that exited before answering, answers with nothing.
pub(crate) struct TsserverReply(Cell<Option<(Value, SharedReplies)>>);

impl TsserverReply {
    fn new(id: Value, replies: &SharedReplies) -> Self {
        Self(Cell::new(Some((id, Arc::clone(replies)))))
    }

    fn send(&self, body: Value) {
        if let Some((id, replies)) = self.0.take() {
            send_reply(&replies, Outgoing::TsserverResponse { id, body });
        }
    }

    /// Moves the answering out of a message the writer only borrows.
    fn take(&self) -> Self {
        Self(Cell::new(self.0.take()))
    }
}

impl Drop for TsserverReply {
    fn drop(&mut self) {
        self.send(Value::Null);
    }
}

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
    /// Asks for the colors `text`, the text the server has, names.
    Colors {
        uri: Uri,
        path: PathBuf,
        text: Snapshot,
    },
    /// Asks what each name of `text`, the text the server has, is.
    SemanticTokens {
        uri: Uri,
        path: PathBuf,
        text: Snapshot,
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
    /// Asks for the signatures of the call at `offset` of `text`; `id` is the
    /// documentation plugin's.
    Signatures {
        uri: Uri,
        id: u64,
        text: Snapshot,
        offset: usize,
        trigger: SignatureTrigger,
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
        /// Only actions of this kind or its subkinds.
        kind: Option<&'static str>,
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
    /// Asks for the edits that format `range` of `text`, or all of it, indented by `indent`
    /// where the project's own settings do not say; `id` is the format plugin's.
    Format {
        uri: Uri,
        id: u64,
        text: Snapshot,
        range: Option<Range<usize>>,
        indent: IndentUnit,
    },
    /// Asks what renaming files, from and to, would change in others.
    WillRename {
        id: u64,
        files: Vec<(Uri, Uri)>,
    },
    /// Tells that files were renamed, from and to.
    DidRename {
        files: Vec<(Uri, Uri)>,
    },
    /// Asks for the project's symbols matching `query`; `id` is xref's.
    Symbols {
        id: u64,
        query: String,
    },
    /// Asks for the symbols of the file at `uri`; `id` is xref's.
    DocumentSymbols {
        id: u64,
        uri: Uri,
    },
    /// Asks for the name of the symbol at `offset` of `text` to rename, or whether it can
    /// be; `id` is the rename plugin's.
    PrepareRename {
        id: u64,
        uri: Uri,
        text: Snapshot,
        offset: usize,
    },
    /// Asks for the edits renaming the symbol at `offset` of `text` makes.
    Rename {
        id: u64,
        uri: Uri,
        text: Snapshot,
        offset: usize,
        new_name: String,
    },
    /// Answers a request the server made.
    Reply {
        id: Value,
        result: Value,
    },
    /// Asks this TypeScript server what another server asked of it; the answer goes to
    /// that one, by `reply`.
    Tsserver {
        request: TsserverRequest,
        reply: TsserverReply,
    },
    /// Answers the server's `tsserver/request` `id`.
    TsserverResponse {
        id: Value,
        body: Value,
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

/// What a server answered about renaming a symbol.
pub(crate) enum SymbolRenamed {
    Name {
        id: u64,
        name: Result<String, String>,
    },
    /// None when there is nothing to change.
    Edit {
        id: u64,
        edit: Result<Option<WorkspaceEdit>, String>,
        encoding: Encoding,
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
    References,
}

impl Question {
    fn method(self) -> &'static str {
        match self {
            Question::Hover => HoverRequest::METHOD,
            Question::Definition => GotoDefinition::METHOD,
            Question::References => References::METHOD,
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
    /// Definitions or references: positions in the target files, which only the editor
    /// has the texts of.
    Locations {
        id: u64,
        targets: Vec<(PathBuf, Position)>,
        encoding: Encoding,
    },
    Signatures {
        id: u64,
        signatures: Option<Signatures>,
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
    Pull { path: PathBuf, version: i32 },
    Colors { path: PathBuf, text: Snapshot },
    SemanticTokens { path: PathBuf, text: Snapshot },
    Complete { id: u64, origin: Origin },
    Resolve { id: u64, index: usize, origin: Origin },
    AtPoint { question: Question, id: u64, origin: Origin },
    Signatures { id: u64, mode: &'static str },
    CodeActions { id: u64, kind: Option<&'static str> },
    ResolveAction { id: u64 },
    Execute { id: u64 },
    Format { id: u64, text: Snapshot },
    WillRename { id: u64 },
    Symbols { id: u64 },
    DocumentSymbols { id: u64, path: PathBuf },
    PrepareRename { id: u64, text: Snapshot, offset: usize },
    Rename { id: u64 },
    Tsserver(TsserverReply),
}

impl Pending {
    /// Whether a failure is answered too, so the user hears of it, or a list waiting for
    /// every server's completions is not held up; other failed requests, e.g. cancelled
    /// by a newer edit, are dropped.
    fn answers_failure(&self) -> bool {
        !matches!(
            self,
            Pending::Pull { .. }
                | Pending::Colors { .. }
                | Pending::SemanticTokens { .. }
                | Pending::Resolve { .. }
        )
    }
}

/// What the reader thread collects for the editor.
#[derive(Default)]
struct Inbox {
    published: HashMap<PathBuf, Published>,
    /// The newest colors of each file and the text they were found in.
    colors: HashMap<PathBuf, (Snapshot, Vec<Swatch>)>,
    /// Likewise its semantic tokens, with the legend that names them.
    semantic_tokens: HashMap<PathBuf, SemanticTokens>,
    /// Bumped when the server asks for every file's tokens again, e.g. once it loaded.
    semantic_refreshes: u64,
    requests: HashMap<u64, Pending>,
    answers: Vec<Answer>,
    /// Kept apart from `answers`, which are for the window buffer's document.
    renamed: Vec<Renamed>,
    found: Vec<Found>,
    document_symbols: Vec<Found>,
    symbol_renamed: Vec<SymbolRenamed>,
    tsserver_requests: Vec<TsserverRequest>,
    /// Why the server exited, once it has.
    exit: Option<String>,
}

type SharedInbox = Arc<Mutex<Inbox>>;

/// A file's semantic tokens, the text they were found in and the legend naming them.
pub(crate) type SemanticTokens = (Snapshot, Arc<Legend>, Vec<(Range<usize>, Token)>);

fn lock(inbox: &SharedInbox) -> MutexGuard<'_, Inbox> {
    inbox.lock().expect("no thread panics holding the inbox")
}

type SharedProgress = Arc<Mutex<Progress>>;

fn lock_progress(progress: &SharedProgress) -> MutexGuard<'_, Progress> {
    progress.lock().expect("no thread panics holding the progress")
}

/// The reader's way to the writer, taken when the client is dropped: the writer ends
/// once no one can send to it, which closes the server's input so that it exits.
type SharedReplies = Arc<Mutex<Option<Sender<Outgoing>>>>;

fn send_reply(replies: &SharedReplies, message: Outgoing) {
    let replies = replies.lock().expect("no thread panics holding the replies");
    if let Some(replies) = &*replies {
        let _ = replies.send(message);
    }
}

/// Lines of a server's stderr kept to say why it exited.
const KEPT_ERROR_LINES: usize = 3;

/// The last non-empty lines of `stderr`, read to its end.
fn last_lines(stderr: ChildStderr) -> Vec<String> {
    let mut kept = VecDeque::with_capacity(KEPT_ERROR_LINES);
    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if kept.len() == KEPT_ERROR_LINES {
            kept.pop_front();
        }
        kept.push_back(line.to_owned());
    }
    kept.into()
}

/// What the server last wrote to stderr, else how it ended.
fn exit_reason(status: io::Result<ExitStatus>, errors: &[String]) -> String {
    match status {
        _ if !errors.is_empty() => format!("exited: {}", errors.join(" ")),
        Ok(status) => format!("exited ({status})"),
        Err(error) => format!("exited: {error}"),
    }
}

/// How the server wants positions and changes and what it offers, agreed on in
/// `initialize`.
struct Negotiated {
    encoding: Encoding,
    incremental: bool,
    completion: Option<CompletionSupport>,
    colors: bool,
    semantic_tokens: bool,
    will_rename: bool,
    did_rename: bool,
    formatting: bool,
    range_formatting: bool,
    workspace_symbols: bool,
    document_symbols: bool,
    rename: bool,
    prepare_rename: bool,
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
    signatures: SharedSignatureSupport,
    /// What the server is busy with; servers that never say they are loading the project
    /// are taken as settled.
    progress: SharedProgress,
    replies: SharedReplies,
    tsserver: Option<&'static str>,
}

impl Drop for Client {
    fn drop(&mut self) {
        self.replies.lock().expect("no thread panics holding the replies").take();
    }
}

impl Client {
    /// Runs `program` as `server`, with `env` added to its environment.
    pub fn start(
        server: &LanguageServer,
        program: &Path,
        env: &[(String, String)],
        root: &Path,
        notifier: Notifier,
    ) -> io::Result<Self> {
        let mut child = Command::new(program)
            .args(server.args)
            .envs(env.iter().map(|(name, value)| (name, value)))
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        // Drained all along, so that a talkative server never blocks on a full pipe.
        let last_errors = thread::Builder::new()
            .name(format!("{} errors", server.name))
            .spawn(move || last_lines(stderr))?;
        let (outgoing, queued) = mpsc::channel();
        let (agreed, negotiated) = mpsc::channel();
        let inbox = SharedInbox::default();
        let pull_generation = Arc::new(AtomicU64::new(0));
        let progress = Arc::new(Mutex::new(Progress::new(Instant::now(), !server.reports_loading)));
        let signatures = SharedSignatureSupport::default();
        let writer = Writer {
            server: server.name,
            stdin: BufWriter::new(stdin),
            inbox: Arc::clone(&inbox),
            notifier: Arc::clone(&notifier),
            next_id: INITIALIZE_ID + 1,
            signatures: Arc::clone(&signatures),
        };
        let program_dir = fs::canonicalize(program).unwrap_or_else(|_| program.to_path_buf());
        let program_dir = program_dir.parent().unwrap_or(&program_dir).to_string_lossy();
        let initialize = initialize_params(
            server.initialization_options.clone().map(|mut options| {
                fill_program_dir(&mut options, &program_dir);
                options
            }),
            server.configuration.is_some(),
            root,
        );
        thread::Builder::new()
            .name(format!("{} writer", server.name))
            .spawn(move || writer.write_all(initialize, negotiated, queued))?;
        let replies = Arc::new(Mutex::new(Some(outgoing.clone())));
        let reader = Reader {
            server: server.name,
            configuration: server.configuration.clone().unwrap_or_default(),
            replies: Arc::clone(&replies),
            agreed: Some(agreed),
            encoding: Encoding::Utf16,
            inbox: Arc::clone(&inbox),
            pull_generation: Arc::clone(&pull_generation),
            notifier: Arc::clone(&notifier),
            progress: Arc::clone(&progress),
            signatures: Arc::clone(&signatures),
            legend: None,
        };
        let (exit_inbox, exit_notifier) = (Arc::clone(&inbox), Arc::clone(&notifier));
        thread::Builder::new().name(format!("{} reader", server.name)).spawn(move || {
            reader.read_all(stdout);
            let status = child.wait();
            let errors = last_errors.join().unwrap_or_default();
            lock(&exit_inbox).exit = Some(exit_reason(status, &errors));
            exit_notifier();
        })?;
        Ok(Self {
            outgoing,
            inbox,
            pulls_diagnostics: server.pull_diagnostics,
            pull_generation,
            notifier,
            executing: Cell::new(None),
            progress,
            signatures,
            replies,
            tsserver: server.tsserver,
        })
    }

    /// The server its `tsserver/request`s go to.
    pub fn tsserver(&self) -> Option<&'static str> {
        self.tsserver
    }

    pub fn take_tsserver_requests(&self) -> Vec<TsserverRequest> {
        std::mem::take(&mut lock(&self.inbox).tsserver_requests)
    }

    /// Asks this TypeScript server what `from` asked of it.
    pub fn ask_tsserver(&self, request: TsserverRequest, from: &Client) {
        let reply = TsserverReply::new(request.id.clone(), &from.replies);
        self.send(Outgoing::Tsserver { request, reply });
    }

    /// Answers its `tsserver/request` `id` with nothing, as there is no TypeScript server
    /// to ask.
    pub fn answer_tsserver(&self, id: Value) {
        self.send(Outgoing::TsserverResponse { id, body: Value::Null });
    }

    pub fn is_settled(&self) -> bool {
        lock_progress(&self.progress).is_settled()
    }

    /// What the server, `name`, is busy with, if it is.
    pub fn busy_status(&self, name: &str) -> Option<StatusItem> {
        lock_progress(&self.progress).status(name)
    }

    /// How long the server took to start and load, once it first has.
    pub fn take_ready(&self) -> Option<Duration> {
        lock_progress(&self.progress).take_ready()
    }

    /// Whether a request for `trigger` may show signatures; true until the server agreed
    /// on what it supports, as the writer holds requests until then.
    pub fn may_sign(&self, trigger: SignatureTrigger) -> bool {
        self.signatures.get().is_none_or(|support| {
            support.as_ref().is_some_and(|support| support.context(trigger).is_some())
        })
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

    pub fn take_colors(&self, path: &Path) -> Option<(Snapshot, Vec<Swatch>)> {
        lock(&self.inbox).colors.remove(path)
    }

    pub fn take_semantic_tokens(&self, path: &Path) -> Option<SemanticTokens> {
        lock(&self.inbox).semantic_tokens.remove(path)
    }

    /// How many times the server asked for every file's tokens again.
    pub fn semantic_refreshes(&self) -> u64 {
        lock(&self.inbox).semantic_refreshes
    }

    /// Why the server exited, the first time it is asked after it has.
    pub fn take_exit(&self) -> Option<String> {
        lock(&self.inbox).exit.take()
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

    pub fn take_document_symbols(&self) -> Vec<Found> {
        std::mem::take(&mut lock(&self.inbox).document_symbols)
    }

    pub fn take_symbol_renamed(&self) -> Vec<SymbolRenamed> {
        std::mem::take(&mut lock(&self.inbox).symbol_renamed)
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

fn fill_program_dir(value: &mut Value, program_dir: &str) {
    match value {
        Value::String(text) => *text = text.replace(PROGRAM_DIR, program_dir),
        Value::Array(items) => {
            items.iter_mut().for_each(|item| fill_program_dir(item, program_dir))
        }
        Value::Object(fields) => {
            fields.values_mut().for_each(|field| fill_program_dir(field, program_dir))
        }
        _ => {}
    }
}

/// Only servers given a configuration are told they may ask for it: others then keep to
/// their defaults, as some take the answer, `null`, for settings.
fn initialize_params(options: Option<Value>, configured: bool, root: &Path) -> Value {
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
            // Servers then say what they are busy with, as `$/progress`.
            "window": { "workDoneProgress": true },
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
                "signatureHelp": {
                    "signatureInformation": {
                        "parameterInformation": { "labelOffsetSupport": true },
                        "activeParameterSupport": true,
                    },
                    "contextSupport": true,
                },
                "colorProvider": {},
                // Whole files only; servers may leave what tree-sitter paints anyway.
                "semanticTokens": {
                    "requests": { "full": true },
                    "tokenTypes": semantic::CAPTURES
                        .iter()
                        .map(|(kind, _)| *kind)
                        .filter(|kind| !kind.contains('.'))
                        .collect::<Vec<_>>(),
                    "tokenModifiers": [
                        "declaration", "definition", "readonly", "static", "deprecated",
                        "abstract", "async", "modification", "documentation", "defaultLibrary",
                    ],
                    "formats": ["relative"],
                    "overlappingTokenSupport": false,
                    "multilineTokenSupport": false,
                    "augmentsSyntaxTokens": true,
                },
                "definition": { "linkSupport": true },
                "references": {},
                "rename": { "prepareSupport": true },
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
                "configuration": configured,
                "applyEdit": true,
                // Renaming a Rust module renames its file too.
                "workspaceEdit": {
                    "documentChanges": true,
                    "resourceOperations": ["create", "rename"],
                    "failureHandling": "abort",
                },
                "fileOperations": { "willRename": true, "didRename": true },
                "semanticTokens": { "refreshSupport": true },
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
    signatures: SharedSignatureSupport,
}

/// What the server agreed to about signatures, once `initialize` is answered; the editor
/// reads it too, so keystrokes that open no call never leave it.
type SharedSignatureSupport = Arc<OnceLock<Option<SignatureSupport>>>;

impl Writer {
    /// The server must hear nothing but `initialize` until it answers, so the queue waits.
    fn write_all(
        mut self,
        initialize: Value,
        negotiated: Receiver<Negotiated>,
        queued: Receiver<Outgoing>,
    ) -> io::Result<()> {
        write_message(&mut self.stdin, &request(INITIALIZE_ID, Initialize::METHOD, initialize))?;
        let Ok(negotiated) = negotiated.recv() else {
            return Ok(());
        };
        write_message(&mut self.stdin, &notification::<Initialized>(InitializedParams {}))?;
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
                let pending = Pending::Pull { path: path.clone(), version: *version };
                self.ask(DocumentDiagnosticRequest::METHOD, params, pending)
            }
            Outgoing::Colors { uri, path, text } => {
                if !negotiated.colors {
                    return Ok(());
                }
                let params = json!({ "textDocument": TextDocumentIdentifier::new(uri.clone()) });
                let pending = Pending::Colors { path: path.clone(), text: text.clone() };
                self.ask(DocumentColor::METHOD, params, pending)
            }
            Outgoing::SemanticTokens { uri, path, text } => {
                if !negotiated.semantic_tokens {
                    return Ok(());
                }
                let params = json!({ "textDocument": TextDocumentIdentifier::new(uri.clone()) });
                let pending = Pending::SemanticTokens { path: path.clone(), text: text.clone() };
                self.ask(SemanticTokensFullRequest::METHOD, params, pending)
            }
            Outgoing::Complete { uri, id, text, start, offset, trigger, modes } => {
                let Some(context) = completion.and_then(|support| support.context(*trigger)) else {
                    let origin = origin(text, *start, *modes);
                    let (list, items) = completion::list(None, &origin);
                    lock(&self.inbox).answers.push(Answer::Listed { id: *id, list, items, origin });
                    (self.notifier)();
                    return Ok(());
                };
                let params = json!({
                    "textDocument": TextDocumentIdentifier::new(uri.clone()),
                    "position": to_lsp(text, *offset, negotiated.encoding),
                    "context": context,
                });
                let pending = Pending::Complete { id: *id, origin: origin(text, *start, *modes) };
                self.ask(Completion::METHOD, params, pending)
            }
            Outgoing::Resolve { id, index, item, text, start, modes } => {
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
            Outgoing::AtPoint { question, uri, id, text, offset, modes } => {
                let mut params = json!({
                    "textDocument": TextDocumentIdentifier::new(uri.clone()),
                    "position": to_lsp(text, *offset, negotiated.encoding),
                });
                if *question == Question::References {
                    params["context"] = json!({ "includeDeclaration": true });
                }
                let pending = Pending::AtPoint {
                    question: *question,
                    id: *id,
                    origin: origin(text, *offset, *modes),
                };
                self.ask(question.method(), params, pending)
            }
            Outgoing::Signatures { uri, id, text, offset, trigger, modes } => {
                let support = self.signatures.get().and_then(Option::as_ref);
                let Some(context) = support.and_then(|support| support.context(*trigger)) else {
                    lock(&self.inbox)
                        .answers
                        .push(Answer::Signatures { id: *id, signatures: None });
                    (self.notifier)();
                    return Ok(());
                };
                let params = json!({
                    "textDocument": TextDocumentIdentifier::new(uri.clone()),
                    "position": to_lsp(text, *offset, negotiated.encoding),
                    "context": context,
                });
                let pending = Pending::Signatures { id: *id, mode: modes.document };
                self.ask(SignatureHelpRequest::METHOD, params, pending)
            }
            Outgoing::CodeActions { uri, id, text, range, diagnostics, kind } => {
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
                let mut params = json!({
                    "textDocument": TextDocumentIdentifier::new(uri.clone()),
                    "range": range,
                    "context": { "diagnostics": diagnostics, "triggerKind": 1 },
                });
                if let Some(kind) = kind {
                    params["context"]["only"] = json!([kind]);
                }
                self.ask(
                    CodeActionRequest::METHOD,
                    params,
                    Pending::CodeActions { id: *id, kind: *kind },
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
            Outgoing::Tsserver { request, reply } => {
                // As VS Code asks it, which Vue's server is written for.
                let params = json!({
                    "command": "typescript.tsserverRequest",
                    "arguments": [request.command, request.args, { "isAsync": true, "lowPriority": true }],
                });
                self.ask(ExecuteCommand::METHOD, params, Pending::Tsserver(reply.take()))
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
            Outgoing::DidRename { files } => {
                if !negotiated.did_rename {
                    return Ok(());
                }
                let files: Vec<_> = files
                    .iter()
                    .map(|(old, new)| json!({ "oldUri": old, "newUri": new }))
                    .collect();
                json!({ "jsonrpc": "2.0", "method": DidRenameFiles::METHOD, "params": { "files": files } })
            }
            Outgoing::PrepareRename { id, uri, text, offset } => {
                if !negotiated.rename || !negotiated.prepare_rename {
                    let name = if negotiated.rename {
                        symbol_rename::word_at(text, *offset).ok_or_else(nothing_to_rename)
                    } else {
                        Err(format!("{} cannot rename symbols", self.server))
                    };
                    let answer = SymbolRenamed::Name { id: *id, name };
                    lock(&self.inbox).symbol_renamed.push(answer);
                    (self.notifier)();
                    return Ok(());
                }
                let params = json!({
                    "textDocument": TextDocumentIdentifier::new(uri.clone()),
                    "position": to_lsp(text, *offset, negotiated.encoding),
                });
                let pending =
                    Pending::PrepareRename { id: *id, text: text.clone(), offset: *offset };
                self.ask(PrepareRenameRequest::METHOD, params, pending)
            }
            Outgoing::Rename { id, uri, text, offset, new_name } => {
                let params = json!({
                    "textDocument": TextDocumentIdentifier::new(uri.clone()),
                    "position": to_lsp(text, *offset, negotiated.encoding),
                    "newName": new_name,
                });
                self.ask(Rename::METHOD, params, Pending::Rename { id: *id })
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
            Outgoing::DocumentSymbols { id, uri } => {
                let path = uri::to_path(uri).unwrap_or_default();
                if !negotiated.document_symbols {
                    lock(&self.inbox).document_symbols.push(Found {
                        id: *id,
                        symbols: Vec::new(),
                        encoding: negotiated.encoding,
                        settled: true,
                    });
                    (self.notifier)();
                    return Ok(());
                }
                let params = json!({ "textDocument": TextDocumentIdentifier::new(uri.clone()) });
                let pending = Pending::DocumentSymbols { id: *id, path };
                self.ask(DocumentSymbolRequest::METHOD, params, pending)
            }
            Outgoing::Format { uri, id, text, range, indent } => {
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
                let mut params = json!({
                    "textDocument": TextDocumentIdentifier::new(uri.clone()),
                    "options": format_options(*indent),
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
                let pending = Pending::Format { id: *id, text: text.clone() };
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

/// Formatters follow the project's own settings, such as rustfmt.toml; those without, like
/// TypeScript's, these. A tab is as wide as the display draws it.
fn format_options(indent: IndentUnit) -> Value {
    let (tab_size, insert_spaces) = match indent {
        IndentUnit::Tabs => (4, false),
        IndentUnit::Spaces(width) => (width, true),
    };
    json!({ "tabSize": tab_size, "insertSpaces": insert_spaces, "insertFinalNewline": true })
}

/// A pull, completion or question that a later one in the same batch repeats is not worth
/// the server's time.
fn is_superseded(message: &Outgoing, later: &[Outgoing]) -> bool {
    later.iter().any(|other| match (message, other) {
        (Outgoing::Pull { uri, .. }, Outgoing::Pull { uri: other, .. })
        | (Outgoing::Colors { uri, .. }, Outgoing::Colors { uri: other, .. })
        | (Outgoing::SemanticTokens { uri, .. }, Outgoing::SemanticTokens { uri: other, .. })
        | (Outgoing::Complete { uri, .. }, Outgoing::Complete { uri: other, .. })
        | (Outgoing::Signatures { uri, .. }, Outgoing::Signatures { uri: other, .. })
        | (Outgoing::CodeActions { uri, .. }, Outgoing::CodeActions { uri: other, .. }) => {
            uri == other
        }
        (Outgoing::AtPoint { question, .. }, Outgoing::AtPoint { question: other, .. }) => {
            question == other
        }
        (Outgoing::Symbols { .. }, Outgoing::Symbols { .. }) => true,
        (Outgoing::DocumentSymbols { uri, .. }, Outgoing::DocumentSymbols { uri: other, .. }) => {
            uri == other
        }
        _ => false,
    })
}

impl Outgoing {
    fn to_json(&self, negotiated: &Negotiated) -> Value {
        match self {
            Outgoing::Open { uri, language, version, text } => {
                notification::<DidOpenTextDocument>(DidOpenTextDocumentParams {
                    text_document: TextDocumentItem::new(
                        uri.clone(),
                        (*language).into(),
                        *version,
                        contents(text, 0..text.len()),
                    ),
                })
            }
            Outgoing::Change { uri, version, old, new, edit } => {
                notification::<DidChangeTextDocument>(DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier::new(uri.clone(), *version),
                    content_changes: vec![content_change(old, new, *edit, negotiated)],
                })
            }
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
            | Outgoing::Colors { .. }
            | Outgoing::SemanticTokens { .. }
            | Outgoing::Complete { .. }
            | Outgoing::Resolve { .. }
            | Outgoing::AtPoint { .. }
            | Outgoing::Signatures { .. }
            | Outgoing::CodeActions { .. }
            | Outgoing::ResolveAction { .. }
            | Outgoing::Execute { .. }
            | Outgoing::Format { .. }
            | Outgoing::WillRename { .. }
            | Outgoing::DidRename { .. }
            | Outgoing::Symbols { .. }
            | Outgoing::DocumentSymbols { .. }
            | Outgoing::PrepareRename { .. }
            | Outgoing::Rename { .. }
            | Outgoing::Tsserver { .. } => unreachable!("the writer numbers requests"),
            Outgoing::Reply { id, result } => {
                json!({ "jsonrpc": "2.0", "id": id, "result": result })
            }
            Outgoing::TsserverResponse { id, body } => {
                json!({ "jsonrpc": "2.0", "method": TSSERVER_RESPONSE, "params": [[id, body]] })
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

fn nothing_to_rename() -> String {
    "Nothing to rename here".into()
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
    replies: SharedReplies,
    /// Taken once `initialize` is answered.
    agreed: Option<Sender<Negotiated>>,
    encoding: Encoding,
    inbox: SharedInbox,
    pull_generation: Arc<AtomicU64>,
    notifier: Notifier,
    progress: SharedProgress,
    signatures: SharedSignatureSupport,
    /// What the server's semantic token numbers stand for, once `initialize` is answered.
    legend: Option<Arc<Legend>>,
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
        // Answers what another server asked with nothing now, not once the client goes.
        let requests = std::mem::take(&mut lock(&self.inbox).requests);
        drop(requests);
    }

    fn handle(&mut self, mut message: Value) {
        let params = message["params"].take();
        let id = message.get_mut("id").map(Value::take);
        match (message["method"].as_str(), id) {
            (Some(method), Some(id)) => self.answer(method, &params, id),
            (Some(PublishDiagnostics::METHOD), None) => self.publish(params),
            (Some(TSSERVER_REQUEST), None) => self.ask_tsserver(params),
            (Some(SERVER_STATUS), None) => {
                let settled = params["quiescent"].as_bool().unwrap_or(true);
                let changed = lock_progress(&self.progress).settle(settled, Instant::now());
                if changed {
                    (self.notifier)();
                }
            }
            (Some(PROGRESS), None) => {
                let changed = lock_progress(&self.progress).report(&params, Instant::now());
                if changed {
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
            SemanticTokensRefresh::METHOD => {
                lock(&self.inbox).semantic_refreshes += 1;
                (self.notifier)();
                Value::Null
            }
            _ => Value::Null,
        };
        send_reply(&self.replies, Outgoing::Reply { id, result });
    }

    /// Left for the editor to pass on to the project's TypeScript server.
    fn ask_tsserver(&mut self, params: Value) {
        let request = match params {
            Value::Array(mut wrapped) if wrapped.len() == 1 => wrapped.remove(0),
            params => params,
        };
        let Ok((id, command, args)) = serde_json::from_value(request) else {
            return;
        };
        let request = TsserverRequest { id, command, args };
        lock(&self.inbox).tsserver_requests.push(request);
        (self.notifier)();
    }

    fn initialized(&mut self, response: &Value) {
        let Some(agreed) = self.agreed.take() else {
            return;
        };
        if let Some(error) = response.get("error") {
            eprintln!("remacs: {} failed to initialize: {error}", self.server);
            return;
        }
        // Only after the error: a server failing to start is not ready, and soon exits.
        if lock_progress(&self.progress).initialized(Instant::now()) {
            (self.notifier)();
        }
        let capabilities = &response["result"]["capabilities"];
        self.encoding = if capabilities["positionEncoding"] == "utf-8" {
            Encoding::Utf8
        } else {
            Encoding::Utf16
        };
        let sync = &capabilities["textDocumentSync"];
        let full_sync = sync.get("change").unwrap_or(sync).as_u64() == Some(1);
        let _ = (self.signatures).set(SignatureSupport::from_capabilities(capabilities));
        self.legend = Legend::offered(capabilities).map(Arc::new);
        let _ = agreed.send(Negotiated {
            encoding: self.encoding,
            incremental: !full_sync,
            completion: CompletionSupport::from_capabilities(capabilities),
            colors: is_provided(&capabilities["colorProvider"]),
            semantic_tokens: self.legend.is_some(),
            will_rename: capabilities["workspace"]["fileOperations"]["willRename"].is_object(),
            did_rename: capabilities["workspace"]["fileOperations"]["didRename"].is_object(),
            formatting: is_provided(&capabilities["documentFormattingProvider"]),
            range_formatting: is_provided(&capabilities["documentRangeFormattingProvider"]),
            workspace_symbols: is_provided(&capabilities["workspaceSymbolProvider"]),
            document_symbols: is_provided(&capabilities["documentSymbolProvider"]),
            rename: is_provided(&capabilities["renameProvider"]),
            prepare_rename: capabilities["renameProvider"]["prepareProvider"] == true,
        });
    }

    /// A failed question is answered with nothing, so the user hears that there is
    /// nothing to show; a failed action with why.
    fn answered(&mut self, id: u64, mut response: Value) {
        let Some(pending) = lock(&self.inbox).requests.remove(&id) else {
            return;
        };
        let error = response
            .get("error")
            .map(|error| error["message"].as_str().unwrap_or("no reason given").to_owned());
        if error.is_some() && !pending.answers_failure() {
            return;
        }
        let mut result = response["result"].take();
        match pending {
            Pending::Pull { path, version } => self.pulled(path, version, result),
            Pending::Colors { path, text } => {
                let found = serde_json::from_value(result).unwrap_or_default();
                let swatches = color::swatches(found, &text, self.encoding);
                lock(&self.inbox).colors.insert(path, (text, swatches));
                (self.notifier)();
            }
            Pending::SemanticTokens { path, text } => {
                let Some(legend) = &self.legend else {
                    return;
                };
                let data: Vec<u32> =
                    serde_json::from_value(result["data"].take()).unwrap_or_default();
                let tokens = semantic::decode(&data, &text, self.encoding);
                let found = (text, Arc::clone(legend), tokens);
                lock(&self.inbox).semantic_tokens.insert(path, found);
                (self.notifier)();
            }
            Pending::Complete { id, origin } => {
                let response = serde_json::from_value(result).unwrap_or_default();
                let (list, items) = completion::list(response, &origin);
                self.deliver(Answer::Listed { id, list, items, origin });
            }
            Pending::Resolve { id, index, origin } => {
                if let Ok(item) = serde_json::from_value(result) {
                    let item = completion::convert(&item, &origin);
                    self.deliver(Answer::Resolved { id, index, item });
                }
            }
            Pending::AtPoint { question: Question::Hover, id, origin } => {
                let response = serde_json::from_value(result).unwrap_or_default();
                let answer = hover::answer(response, &origin.text, origin.encoding, origin.modes);
                self.deliver(Answer::Hover { id, answer });
            }
            Pending::AtPoint { question: Question::Definition, id, .. } => {
                let response = serde_json::from_value(result).unwrap_or_default();
                self.deliver(Answer::Locations {
                    id,
                    targets: definition::targets(response),
                    encoding: self.encoding,
                });
            }
            Pending::AtPoint { question: Question::References, id, .. } => {
                let response: Option<Vec<lsp_types::Location>> =
                    serde_json::from_value(result).unwrap_or_default();
                let response = response.map(GotoDefinitionResponse::Array);
                self.deliver(Answer::Locations {
                    id,
                    targets: definition::targets(response),
                    encoding: self.encoding,
                });
            }
            Pending::Signatures { id, mode } => {
                let response = serde_json::from_value(result).unwrap_or_default();
                self.deliver(Answer::Signatures {
                    id,
                    signatures: signature::signatures(response, mode),
                });
            }
            Pending::CodeActions { id, kind } => {
                let mut actions: Vec<CodeActionOrCommand> =
                    serde_json::from_value(result).unwrap_or_default();
                actions.retain(|action| {
                    !matches!(action, CodeActionOrCommand::CodeAction(action) if action.disabled.is_some())
                });
                // Servers may ignore `only`.
                if let Some(kind) = kind {
                    actions.retain(|action| code_action::is_of_kind(action, kind));
                }
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
                        settled: lock_progress(&self.progress).is_settled(),
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
                let edit = error.is_none().then(|| serde_json::from_value(result).ok()).flatten();
                let settled = lock_progress(&self.progress).is_settled();
                lock(&self.inbox).renamed.push(Renamed {
                    id,
                    edit,
                    encoding: self.encoding,
                    settled,
                });
                (self.notifier)();
            }
            Pending::Symbols { id } => {
                let response =
                    error.is_none().then(|| serde_json::from_value(result).ok()).flatten();
                let settled = lock_progress(&self.progress).is_settled();
                lock(&self.inbox).found.push(Found {
                    id,
                    symbols: symbols::found(response),
                    encoding: self.encoding,
                    settled,
                });
                (self.notifier)();
            }
            Pending::DocumentSymbols { id, path } => {
                let response =
                    error.is_none().then(|| serde_json::from_value(result).ok()).flatten();
                lock(&self.inbox).document_symbols.push(Found {
                    id,
                    symbols: document_symbols::found(response, &path),
                    encoding: self.encoding,
                    settled: true,
                });
                (self.notifier)();
            }
            Pending::PrepareRename { id, text, offset } => {
                let name = match error {
                    Some(message) => Err(message),
                    None => serde_json::from_value(result)
                        .ok()
                        .and_then(|response| {
                            symbol_rename::name(response, &text, offset, self.encoding)
                        })
                        .ok_or_else(nothing_to_rename),
                };
                self.deliver_renamed(SymbolRenamed::Name { id, name });
            }
            Pending::Rename { id } => {
                let edit = match error {
                    Some(message) => Err(message),
                    None => Ok(serde_json::from_value(result).ok()),
                };
                let encoding = self.encoding;
                self.deliver_renamed(SymbolRenamed::Edit { id, edit, encoding });
            }
            // The whole tsserver response comes back; a failure is answered with nothing,
            // as the asking server waits for an answer.
            Pending::Tsserver(reply) => {
                reply.send(result.get_mut("body").map(Value::take).unwrap_or_default());
            }
        }
    }

    fn deliver_renamed(&mut self, answer: SymbolRenamed) {
        lock(&self.inbox).symbol_renamed.push(answer);
        (self.notifier)();
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
        let published = Published { version, diagnostics, encoding: self.encoding };
        lock(&self.inbox).published.insert(path, published);
        (self.notifier)();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_server_that_exits_at_once_says_why() {
        // Like TypeScript 5's tsc, which has no `--lsp`.
        let server = LanguageServer {
            name: "tsc",
            command: "sh",
            args: &["-c", "echo \"error TS5023: Unknown compiler option '--lsp'.\" >&2; exit 1"],
            ..LanguageServer::default()
        };
        let (sender, exited) = mpsc::channel();
        let notifier: Notifier = Arc::new(move || {
            let _ = sender.send(());
        });
        let program = Path::new("sh");
        let client = Client::start(&server, program, &[], Path::new("/"), notifier).unwrap();
        exited.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            client.take_exit().as_deref(),
            Some("exited: error TS5023: Unknown compiler option '--lsp'.")
        );
        assert_eq!(client.take_exit(), None);
    }

    #[test]
    fn a_dropped_client_closes_the_servers_input() {
        // Answers `initialize`, asks for the configuration, then serves until its input
        // closes, like oxfmt.
        let initialized = r#"{"jsonrpc":"2.0","id":0,"result":{"capabilities":{}}}"#;
        let request = r#"{"jsonrpc":"2.0","id":"c","method":"workspace/configuration","params":{"items":[]}}"#;
        let script = format!(
            "printf 'Content-Length: {}\\r\\n\\r\\n%s' '{initialized}'; \
             printf 'Content-Length: {}\\r\\n\\r\\n%s' '{request}'; \
             cat > /dev/null",
            initialized.len(),
            request.len(),
        );
        let args: &'static [&'static str] = Vec::leak(vec!["-c", String::leak(script)]);
        let server =
            LanguageServer { name: "oxfmt", command: "sh", args, ..LanguageServer::default() };
        let (sender, notified) = mpsc::channel();
        let notifier: Notifier = Arc::new(move || {
            let _ = sender.send(());
        });
        let client = Client::start(&server, Path::new("sh"), &[], Path::new("/"), notifier);
        let inbox = Arc::clone(&client.unwrap().inbox);
        while lock(&inbox).exit.is_none() {
            notified.recv_timeout(Duration::from_secs(5)).expect("the server exits");
        }
    }

    /// A shell command writing `json` as a message.
    fn message(json: &str) -> String {
        format!("printf 'Content-Length: {}\\r\\n\\r\\n%s' '{json}'", json.len())
    }

    /// A server that answers `initialize`, runs the shell command `then` once its input
    /// holds `after`, and keeps its input in `input`.
    fn fake_server(input: &Path, after: &str, then: &str) -> LanguageServer {
        let initialized = r#"{"jsonrpc":"2.0","id":0,"result":{"capabilities":{}}}"#;
        let input = input.display();
        let script = format!(
            // A background command's input is /dev/null unless taken from another fd.
            "{}; exec 3<&0; cat <&3 > {input} & \
             until grep -q '{after}' {input} 2>/dev/null; do sleep 0.02; done; {}; wait",
            message(initialized),
            then,
        );
        LanguageServer {
            name: "fake",
            args: Vec::leak(vec!["-c", String::leak(script)]),
            ..LanguageServer::default()
        }
    }

    /// Vue's server asks the TypeScript server, which runs `then` once asked; Vue's input
    /// once answered, and the TypeScript server's.
    fn ask_tsserver(test: &str, then: &str) -> (String, String) {
        let directory = std::env::temp_dir().join(format!("remacs-{test}-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let (vue_input, typescript_input) = (directory.join("vue"), directory.join("typescript"));
        let request = r#"{"jsonrpc":"2.0","method":"tsserver/request","params":[[7,"_vue:projectInfo",{"file":"/a.vue"}]]}"#;
        let vue = fake_server(&vue_input, "initialized", &message(request));
        let typescript = fake_server(&typescript_input, "tsserverRequest", then);
        let (sender, notified) = mpsc::channel();
        let notifier: Notifier = Arc::new(move || {
            let _ = sender.send(());
        });
        let start = |server| {
            Client::start(server, Path::new("sh"), &[], Path::new("/"), Arc::clone(&notifier))
                .unwrap()
        };
        let (vue, typescript) = (start(&vue), start(&typescript));

        let requests = loop {
            notified.recv_timeout(Duration::from_secs(5)).unwrap();
            let requests = vue.take_tsserver_requests();
            if !requests.is_empty() {
                break requests;
            }
        };
        for request in requests {
            typescript.ask_tsserver(request, &vue);
        }

        let started = std::time::Instant::now();
        let answered = loop {
            let input = fs::read_to_string(&vue_input).unwrap_or_default();
            if input.contains(TSSERVER_RESPONSE) {
                break input;
            }
            assert!(started.elapsed() < Duration::from_secs(5), "never answered");
            thread::sleep(Duration::from_millis(20));
        };
        let asked = fs::read_to_string(&typescript_input).unwrap();
        let _ = fs::remove_dir_all(&directory);
        (answered, asked)
    }

    #[test]
    fn a_tsserver_request_is_asked_of_the_typescript_server_and_answered() {
        let response = r#"{"jsonrpc":"2.0","id":1,"result":{"type":"response","body":{"configFileName":"/tsconfig.json"}}}"#;
        let (answered, asked) = ask_tsserver("tsserver-answers", &message(response));
        let answer = r#"{"jsonrpc":"2.0","method":"tsserver/response","params":[[7,{"configFileName":"/tsconfig.json"}]]}"#;
        assert!(answered.contains(answer), "{answered}");
        assert!(asked.contains(r#""arguments":["_vue:projectInfo",{"file":"/a.vue"},"#), "{asked}");
    }

    #[test]
    fn a_typescript_server_exiting_unasked_answers_with_nothing() {
        let (answered, _) = ask_tsserver("tsserver-exits", "exit 0");
        let answer = r#"{"jsonrpc":"2.0","method":"tsserver/response","params":[[7,null]]}"#;
        assert!(answered.contains(answer), "{answered}");
    }

    #[test]
    fn a_tsserver_answer_dropped_unsent_answers_with_nothing() {
        let (sender, sent) = mpsc::channel();
        let replies: SharedReplies = Arc::new(Mutex::new(Some(sender)));
        TsserverReply::new(json!(1), &replies).send(json!("body"));
        // E.g. left in the queue of a server that exited before it was initialized.
        drop(TsserverReply::new(json!(2), &replies));
        let answers: Vec<_> = sent
            .try_iter()
            .map(|message| match message {
                Outgoing::TsserverResponse { id, body } => (id, body),
                _ => panic!("not an answer"),
            })
            .collect();
        assert_eq!(answers, [(json!(1), json!("body")), (json!(2), Value::Null)]);
    }

    #[test]
    fn formatting_options_follow_the_buffers_indentation() {
        let options = |tab_size: usize, insert_spaces: bool| json!({ "tabSize": tab_size, "insertSpaces": insert_spaces, "insertFinalNewline": true });
        assert_eq!(format_options(IndentUnit::Spaces(2)), options(2, true));
        assert_eq!(format_options(IndentUnit::Tabs), options(4, false));
    }

    #[test]
    fn the_program_dir_is_filled_into_initialization_options() {
        let mut options =
            json!({ "plugins": [{ "location": "{program-dir}", "languages": ["vue"] }] });
        fill_program_dir(&mut options, "/tools/bin");
        assert_eq!(
            options,
            json!({ "plugins": [{ "location": "/tools/bin", "languages": ["vue"] }] })
        );
    }
}
