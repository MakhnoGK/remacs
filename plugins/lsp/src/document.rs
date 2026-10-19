use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::mem;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use lsp_types::{
    CodeAction, CodeActionOrCommand, CompletionItem, DiagnosticSeverity, NumberOrString, Uri,
};
use remacs_actions::{Action, Request as ActionRequest};
use remacs_completion::{List, Request};
use remacs_core::{Buffer, Editor, Embedded, Face, Notifier, Span};
use remacs_diagnostics::{self as diagnostics, Diagnostic, Severity};
use remacs_documentation::{Answer as DocumentationAnswer, SignatureRequest, SignatureTrigger};
use remacs_format::{Failure, Request as FormatRequest};
use remacs_text::Snapshot;
use remacs_tools::Program;
use remacs_xref::Lookup;
use serde_json::json;

use crate::LanguageServer;
use crate::catalog::{ServerSpec, has_file, project_ancestors};
use crate::client::{Answer, Client, Outgoing, Published, Question};
use crate::color::{self, Swatch};
use crate::completion::Origin;
use crate::embedded::{self, Mirror};
use crate::markdown::CodeModes;
use crate::position::{Encoding, from_lsp};
use crate::settings::LspSettings;
use crate::{code_action, definition, offer, uri};

/// Servers hold whole files in memory and reparse them; larger ones stay unsynced.
const MAX_SYNCED_BYTES: usize = 16 << 20;
/// Sent texts kept for diagnostics that may still arrive for them.
const MAX_PENDING_VERSIONS: usize = 32;

/// A buffer's files on the servers for its mode, in the order the servers are listed.
/// The first answers questions such as where a symbol is defined; all of them check the
/// buffer and complete, document and offer code actions in it, so linters and Tailwind
/// serve beside the language's own server.
#[derive(Default)]
struct Documents {
    list: Vec<Document>,
    /// The newest request of each kind, until every server answered its own.
    actions: Option<Gathering<Vec<Action>>>,
    completions: Option<Gathering<List>>,
    hovers: Option<Gathering<Option<DocumentationAnswer>>>,
}

struct Gathering<T> {
    id: u64,
    /// Per document; `None` until it answered.
    answers: Vec<Option<T>>,
}

impl<T: Default> Gathering<T> {
    /// Waits for the documents asked; the others answered nothing already.
    fn new(id: u64, asked: &[bool]) -> Option<Self> {
        Some(Self {
            id,
            answers: asked
                .iter()
                .map(|&asked| (!asked).then(T::default))
                .collect(),
        })
    }
}

/// Takes document `index`'s answer to request `id`; once every document answered, all
/// their answers in the servers' order. Answers to older requests are dropped.
fn gather<T>(
    gathering: &mut Option<Gathering<T>>,
    id: u64,
    index: usize,
    answer: T,
) -> Option<Vec<T>> {
    let waiting = gathering.as_mut().filter(|gathering| gathering.id == id)?;
    *waiting.answers.get_mut(index)? = Some(answer);
    if waiting.answers.iter().any(Option::is_none) {
        return None;
    }
    let answers = gathering.take()?.answers;
    Some(answers.into_iter().flatten().collect())
}

impl Documents {
    /// Item `index` of completion list `id` as delivered, all servers' items in their
    /// order: the document that listed it and its index there.
    fn listed(&self, id: u64, mut index: usize) -> Option<(&Document, usize)> {
        for document in &self.list {
            let count = document.listed_count(id);
            if index < count {
                return Some((document, index));
            }
            index -= count;
        }
        None
    }

    /// Action `index` of list `id` as delivered, all servers' actions in their order, and
    /// the server that offered it.
    fn offered(
        &self,
        id: u64,
        mut index: usize,
    ) -> Option<(Rc<Client>, CodeActionOrCommand, Encoding)> {
        for document in &self.list {
            let Some(offered) = document.offered.as_ref().filter(|offered| offered.id == id) else {
                continue;
            };
            match offered.actions.get(index) {
                Some(action) => {
                    return Some((
                        Rc::clone(&document.client),
                        action.clone(),
                        offered.encoding,
                    ));
                }
                None => index -= offered.actions.len(),
            }
        }
        None
    }
}

/// A buffer's file as one server knows it, like an eglot-managed buffer, or the code of
/// one language embedded in it. Dropping it, e.g. with the buffer, closes it on the server.
struct Document {
    server: &'static LanguageServer,
    client: Rc<Client>,
    path: PathBuf,
    uri: Uri,
    language: &'static str,
    /// What code in its documentation is highlighted as.
    modes: CodeModes,
    /// For embedded code, the buffer's text as the server has it.
    mirror: Option<Mirror>,
    version: i32,
    modified: bool,
    /// Versions sent and not yet answered with diagnostics, oldest first; the last one is
    /// what the server has now. Each with the buffer's text it was made from.
    pending: VecDeque<(i32, Snapshot, Snapshot)>,
    /// The client's pull generation when the diagnostics were last pulled.
    pulled: Option<u64>,
    /// The colors the file names, as of the buffer's text, and the version last asked.
    colors: Vec<Swatch>,
    colors_asked: Option<i32>,
    /// The newest completion list, as the server sent it, for resolving its items.
    listed: Option<Listed>,
    /// The diagnostics last taken, as the server sent them, and the text they refer to;
    /// code action requests pass them back.
    reported: Option<(Snapshot, Vec<lsp_types::Diagnostic>)>,
    /// The newest code actions, as the server sent them, for carrying one out.
    offered: Option<Offered>,
    /// An action of list `id` the server could not resolve while loading the project,
    /// resolved again once it is done.
    unresolved: Option<(u64, CodeAction)>,
}

struct Listed {
    id: u64,
    items: Vec<CompletionItem>,
    origin: Origin,
}

struct Offered {
    id: u64,
    actions: Vec<CodeActionOrCommand>,
    encoding: Encoding,
}

impl Document {
    fn open(
        want: &Want,
        client: Rc<Client>,
        modes: CodeModes,
        buffer: Snapshot,
        modified: bool,
    ) -> Self {
        let uri = uri::from_path(&want.path);
        let mirror = want
            .embedded
            .as_ref()
            .map(|code| Mirror::new(code.mode, buffer.clone(), code.ranges.clone()));
        let text = mirror.as_ref().map_or_else(|| buffer.clone(), Mirror::text);
        client.send(Outgoing::Open {
            uri: uri.clone(),
            language: want.language,
            version: 0,
            text: text.clone(),
        });
        Self {
            server: want.server,
            client,
            path: want.path.clone(),
            uri,
            language: want.language,
            modes,
            mirror,
            version: 0,
            modified,
            pending: VecDeque::from([(0, text, buffer)]),
            pulled: None,
            colors: Vec::new(),
            colors_asked: None,
            listed: None,
            reported: None,
            offered: None,
            unresolved: None,
        }
    }

    fn serves(&self, want: &Want) -> bool {
        self.server.name == want.server.name
            && self.language == want.language
            && self.path == want.path
    }

    /// The language of what it holds: the buffer's or the embedded code's.
    fn speaks<'a>(&self, buffer_mode: &'a str) -> &'a str {
        self.mirror
            .as_ref()
            .map_or(buffer_mode, |mirror| mirror.mode)
    }

    /// What its problems are listed under.
    fn checker(&self) -> &'static str {
        match &self.mirror {
            Some(mirror) => embedded::checker(self.server.name, mirror.mode),
            None => self.server.name,
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

    /// Sends what changed in `buffer` since the last call, or in its code of the embedded
    /// language as `embedded` tells it; a save shows as the buffer turning unmodified.
    /// Returns the diagnostics published since, with the buffer's text they refer to.
    fn sync(
        &mut self,
        buffer: Snapshot,
        embedded: Option<&[Embedded]>,
        modified: bool,
    ) -> Option<(Snapshot, Vec<Diagnostic>)> {
        let sent = self.sent_text();
        let text = match &mut self.mirror {
            None => buffer.clone(),
            Some(mirror) => {
                let code = embedded
                    .and_then(|embedded| embedded.iter().find(|code| code.mode == mirror.mode));
                // Until the code can be told again, the server keeps what it has.
                code.map_or_else(
                    || sent.clone(),
                    |code| mirror.update(&buffer, code.ranges.clone()),
                )
            }
        };
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
            self.pending.push_back((self.version, text, buffer));
            if self.pending.len() > MAX_PENDING_VERSIONS {
                self.pending.pop_front();
            }
            color::shift(&mut self.colors, edit);
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
        self.update_colors();
        self.resolve_when_settled();
        self.take_diagnostics()
    }

    /// Asks for the colors of each version once, and takes those found, moved onto the
    /// text since.
    fn update_colors(&mut self) {
        let text = self.sent_text();
        if self.colors_asked != Some(self.version) {
            self.colors_asked = Some(self.version);
            self.client.send(Outgoing::Colors {
                uri: self.uri.clone(),
                path: self.path.clone(),
                text: text.clone(),
            });
        }
        if let Some((found_in, mut colors)) = self.client.take_colors(&self.path) {
            if let Some(edit) = text.edit_from(&found_in) {
                color::shift(&mut colors, edit);
            }
            self.colors = colors;
        }
    }

    /// How many items completion list `id` has from this server.
    fn listed_count(&self, id: u64) -> usize {
        self.listed
            .as_ref()
            .filter(|listed| listed.id == id)
            .map_or(0, |listed| listed.items.len())
    }

    fn resolve_when_settled(&mut self) {
        if !self.client.is_settled() {
            return;
        }
        if let Some((id, action)) = self.unresolved.take() {
            self.client.send(Outgoing::ResolveAction {
                id,
                action: Box::new(action),
            });
        }
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
            Some(version) => self
                .pending
                .iter()
                .position(|(sent, ..)| *sent == version)?,
            None => self.pending.len() - 1,
        };
        self.pending.drain(..index);
        let (_, text, buffer) = self.pending[0].clone();
        self.reported = Some((text.clone(), diagnostics.clone()));
        let diagnostics = diagnostics
            .into_iter()
            .filter_map(|diagnostic| convert(diagnostic, &text, encoding))
            // The rest of the file is blank to the server, but a script may still be
            // said to miss what it imports there, so only the code's problems count.
            .filter(|diagnostic| {
                (self.mirror.as_ref()).is_none_or(|mirror| mirror.holds(&diagnostic.range))
            })
            .collect();
        Some((buffer, diagnostics))
    }

    /// Asks for the actions for `range`. The problems are passed back only while the
    /// server has the text they were reported for, since it matches them by position.
    fn ask_actions(&self, id: u64, range: Range<usize>, kind: Option<&'static str>) {
        let text = self.sent_text();
        let diagnostics = self
            .reported
            .as_ref()
            .filter(|(reported, _)| text.edit_from(reported).is_none())
            .map(|(_, diagnostics)| diagnostics.clone())
            .unwrap_or_default();
        self.client.send(Outgoing::CodeActions {
            uri: self.uri.clone(),
            id,
            text,
            range,
            diagnostics,
            kind,
        });
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

    fn ask_signatures(&self, request: SignatureRequest) {
        self.client.send(Outgoing::Signatures {
            uri: self.uri.clone(),
            id: request.id,
            text: self.sent_text(),
            offset: request.offset,
            trigger: request.trigger,
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

    /// Keeps the newest completion list of an answer for resolving, the newest code
    /// actions for carrying one out. Servers such as rust-analyzer answer out of order, so
    /// older actions arriving late do not replace newer ones.
    fn keep(&mut self, answer: &mut Answer) {
        match answer {
            Answer::Listed {
                id, items, origin, ..
            } => {
                self.listed = Some(Listed {
                    id: *id,
                    items: mem::take(items),
                    origin: origin.clone(),
                });
            }
            Answer::CodeActions {
                id,
                actions,
                encoding,
                ..
            } if self.offered.as_ref().is_none_or(|offered| offered.id < *id) => {
                self.offered = Some(Offered {
                    id: *id,
                    actions: mem::take(actions),
                    encoding: *encoding,
                });
            }
            _ => {}
        }
    }
}

impl Documents {
    /// Which of the documents on document `first`'s server, e.g. a page's script and its
    /// TypeScript, an answer of that server is for: the one asked.
    fn answering(&self, first: usize, answer: &Answer) -> usize {
        let client = &self.list[first].client;
        let asked = |index: usize| {
            let document = &self.list[index];
            match answer {
                Answer::Listed { id, .. } => waits(&self.completions, *id, index),
                Answer::Hover { id, .. } => waits(&self.hovers, *id, index),
                Answer::CodeActions { id, .. } => waits(&self.actions, *id, index),
                Answer::Resolved { id, .. } => document.listed_count(*id) > 0,
                Answer::ResolvedAction { id, .. } => {
                    (document.offered.as_ref()).is_some_and(|offered| offered.id == *id)
                }
                _ => false,
            }
        };
        (first..self.list.len())
            .filter(|&index| Rc::ptr_eq(&self.list[index].client, client))
            .find(|&index| asked(index))
            .unwrap_or(first)
    }
}

/// Which of the documents, by the language each speaks, a question about code in
/// `language` goes to: those of that language, else those of the file's own `mode`, as
/// when no server for the code runs, e.g. the HTML server's for CSS; maybe none at all.
fn routed(speakers: &[&str], language: &str, mode: &str) -> Vec<bool> {
    let speaking = |language| speakers.iter().map(move |speaker| *speaker == language);
    if speaking(language).any(|asked| asked) {
        speaking(language).collect()
    } else {
        speaking(mode).collect()
    }
}

/// Whether document `index` is yet to answer request `id`.
fn waits<T>(gathering: &Option<Gathering<T>>, id: u64, index: usize) -> bool {
    gathering.as_ref().is_some_and(|gathering| {
        gathering.id == id && gathering.answers.get(index).is_some_and(Option::is_none)
    })
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
pub(crate) struct Clients {
    running: HashMap<(&'static str, PathBuf), Rc<Client>>,
    /// Servers that could not start; they are not tried again.
    failed: HashSet<&'static str>,
    /// Per server, directories lacking its required files, so they are looked for once.
    declined: HashMap<&'static str, HashSet<PathBuf>>,
    /// Per server, directories of files that wanted it while its program was nowhere.
    missing: HashMap<&'static str, (&'static LanguageServer, HashSet<PathBuf>)>,
    settings: LspSettings,
    /// The servers, once built from the catalog and `lsp.toml`.
    servers: Option<&'static [LanguageServer]>,
    /// What they were built from, to tell which changed.
    specs: BTreeMap<String, ServerSpec>,
}

/// Why a server wanted for a file is not running for it.
pub(crate) enum NotStarted {
    /// Its program is on neither `PATH` nor in `node_modules`, nor downloaded.
    Missing,
    Failed(String),
}

impl Clients {
    /// The running servers and their project roots.
    pub(crate) fn running(&self) -> impl Iterator<Item = (&Path, &Rc<Client>)> {
        self.running
            .iter()
            .map(|((_, root), client)| (root.as_path(), client))
    }

    /// Each server started so far, with the project roots it runs for; none for one that
    /// exited or could not start.
    pub(crate) fn started(&self) -> BTreeMap<&'static str, Vec<&Path>> {
        let mut started: BTreeMap<_, Vec<_>> =
            self.failed.iter().map(|&name| (name, Vec::new())).collect();
        for (name, root) in self.running.keys() {
            started.entry(*name).or_default().push(root.as_path());
        }
        started
    }

    pub(crate) fn settings(&self) -> &LspSettings {
        &self.settings
    }

    pub(crate) fn servers(&self) -> Option<&'static [LanguageServer]> {
        self.servers
    }

    /// Whether `server` is known not to serve `path`: it could not start, its program is
    /// missing, or the file's project is not one it serves.
    pub(crate) fn passes_over(&self, server: &LanguageServer, path: &Path) -> bool {
        let known = |directories: Option<&HashSet<PathBuf>>| {
            path.parent()
                .is_some_and(|directory| directories.is_some_and(|known| known.contains(directory)))
        };
        self.failed.contains(server.name)
            || known(self.declined.get(server.name))
            || known(
                self.missing
                    .get(server.name)
                    .map(|(_, directories)| directories),
            )
    }

    /// The servers whose programs were missing for files in `directory`.
    pub(crate) fn missing_in(
        &self,
        directory: &Path,
    ) -> impl Iterator<Item = &'static LanguageServer> {
        self.missing
            .values()
            .filter(move |(_, directories)| directories.contains(directory))
            .map(|&(server, _)| server)
    }

    /// Looks for server `name`'s program again, e.g. once it was downloaded.
    pub(crate) fn forget_missing(&mut self, name: &str) {
        self.missing.remove(name);
    }

    /// The server for `path`'s project, started on first use; `Err` the one time it
    /// fails to start or is found missing. `None` for a server that failed or is missing,
    /// or that the file's project does not use. `downloaded` is the server's program as
    /// remacs downloaded it, if it did.
    pub(crate) fn client_for(
        &mut self,
        server: &'static LanguageServer,
        path: &Path,
        downloaded: Option<Program>,
        notifier: Notifier,
    ) -> Result<Option<Rc<Client>>, NotStarted> {
        if self.passes_over(server, path) {
            return Ok(None);
        }
        let directory = path.parent().unwrap_or(path);
        if !server.when.holds(directory) {
            let declined = self.declined.entry(server.name).or_default();
            declined.insert(directory.to_path_buf());
            return Ok(None);
        }
        let root = project_root(path, server.root_markers);
        if let Some(client) = self.running.get(&(server.name, root.clone())) {
            return Ok(Some(Rc::clone(client)));
        }
        let Some(Program {
            path: program,
            mut env,
        }) = program(server, &root, downloaded)
        else {
            let (_, missing) = self
                .missing
                .entry(server.name)
                .or_insert_with(|| (server, HashSet::new()));
            missing.insert(directory.to_path_buf());
            return Err(NotStarted::Missing);
        };
        if server.env_when.holds(&root) {
            let own = server
                .env
                .iter()
                .map(|&(name, value)| (name.into(), value.into()));
            env.extend(own);
        }
        match Client::start(server, &program, &env, &root, notifier) {
            Ok(client) => {
                let client = Rc::new(client);
                self.running.insert((server.name, root), Rc::clone(&client));
                Ok(Some(client))
            }
            Err(error) => {
                self.failed.insert(server.name);
                let error = format!("{}: {error}", program.display());
                Err(NotStarted::Failed(error))
            }
        }
    }
}

/// The program set in `lsp.toml`; else the project's own copy in `node_modules` of the
/// root or above, for servers that prefer it; else the command found on `PATH`; else
/// the one remacs downloaded.
fn program(server: &LanguageServer, root: &Path, downloaded: Option<Program>) -> Option<Program> {
    let own = |path: PathBuf| Program {
        path,
        env: Vec::new(),
    };
    if let Some(program) = server.own_program {
        return Some(own(program.to_path_buf()));
    }
    if server.node_modules
        && let Some(program) = root
            .ancestors()
            .find_map(|directory| node_program(&directory.join("node_modules"), server))
    {
        return Some(program);
    }
    remacs_tools::find_on_path(server.command)
        .map(own)
        .or(downloaded)
}

/// `server`'s command in `node_modules/.bin`, else in that of a package its `when` names,
/// where pnpm leaves the programs of the package's own dependencies (Vite+'s oxlint).
/// Its directory goes first on `PATH`, as npx puts it, for the programs it runs in turn
/// (oxlint's tsgolint), unless a `:` in its name keeps it off.
fn node_program(node_modules: &Path, server: &LanguageServer) -> Option<Program> {
    let packages = server
        .when
        .dependencies
        .iter()
        .filter(|package| !package.ends_with('*'))
        .map(|package| node_modules.join(package).join("node_modules"));
    let path = std::iter::once(node_modules.to_path_buf())
        .chain(packages)
        .map(|directory| directory.join(".bin").join(server.command))
        .find(|program| program.is_file())?;
    let bin = path.parent()?.to_path_buf();
    let path_variable = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )));
    let env = path_variable
        .ok()
        .map(|joined| ("PATH".into(), joined.to_string_lossy().into_owned()))
        .into_iter()
        .collect();
    Some(Program { path, env })
}

/// Puts the servers built from `specs` in place, with the `lsp.toml` they came from;
/// returns the names of the servers before them that changed or are gone.
pub(crate) fn replace_catalog(
    editor: &mut Editor,
    settings: LspSettings,
    servers: &'static [LanguageServer],
    specs: BTreeMap<String, ServerSpec>,
) -> Vec<&'static str> {
    let clients = editor.plugin_state::<Clients>();
    let changed = clients
        .servers
        .unwrap_or_default()
        .iter()
        .filter(|server| clients.specs.get(server.name) != specs.get(server.name))
        .map(|server| server.name)
        .collect();
    clients.settings = settings;
    clients.servers = Some(servers);
    clients.specs = specs;
    changed
}

/// Stops server `name` everywhere and forgets that it failed or was missing; the next
/// sync starts it again where it is wanted. Its files are closed and the processes, left
/// without clients, exit as their input closes.
pub(crate) fn restart(editor: &mut Editor, name: &'static str) {
    let clients = editor.plugin_state::<Clients>();
    clients.running.retain(|(server, _), _| *server != name);
    clients.failed.remove(name);
    clients.missing.remove(name);
    detach(editor, name);
}

/// Servers that exited by themselves are not started again, and the editor says why.
fn report_exits(editor: &mut Editor) {
    let clients = editor.plugin_state::<Clients>();
    let mut exited = Vec::new();
    clients
        .running
        .retain(|&(name, _), client| match client.take_exit() {
            Some(reason) => {
                exited.push((name, reason));
                false
            }
            None => true,
        });
    for (name, _) in &exited {
        clients.failed.insert(name);
    }
    for (name, reason) in exited {
        detach(editor, name);
        editor.set_error(format!("{name} {reason}"));
    }
}

/// Closes server `name`'s files in every buffer and drops its diagnostics.
fn detach(editor: &mut Editor, name: &'static str) {
    for buffer in editor.buffers_mut() {
        let Some(documents) = buffer.local_mut::<Documents>() else {
            continue;
        };
        let mut checkers = Vec::new();
        documents.list.retain(|document| {
            let closed = document.server.name == name;
            if closed {
                checkers.push(document.checker());
            }
            !closed
        });
        if checkers.is_empty() {
            continue;
        }
        if documents.list.is_empty() {
            buffer.remove_local::<Documents>();
        }
        for checker in checkers {
            diagnostics::clear(buffer, checker);
        }
    }
}

/// The colors the buffer's servers found in `range`; where two found one, the first's.
pub(crate) fn swatch_spans(buffer: &Buffer, range: Range<usize>) -> Vec<Span> {
    let Some(documents) = buffer.local::<Documents>() else {
        return Vec::new();
    };
    let mut spans: Vec<_> = documents
        .list
        .iter()
        .flat_map(|document| &document.colors)
        .filter(|(color, _)| color.start < range.end && color.end > range.start)
        .map(|(color, rgba)| Span {
            range: color.clone(),
            face: Face::Swatch(*rgba),
        })
        .collect();
    spans.sort_by_key(|span| span.range.start);
    let mut end = 0;
    spans.retain(|span| {
        let keep = span.range.start >= end;
        if keep {
            end = span.range.end;
        }
        keep
    });
    spans
}

/// The servers attached to the window buffer.
pub(crate) fn window_servers(editor: &Editor) -> Vec<&'static str> {
    editor
        .window_buffer()
        .local::<Documents>()
        .map(|documents| {
            documents
                .list
                .iter()
                .map(|document| document.server.name)
                .collect()
        })
        .unwrap_or_default()
}

/// The server that answers questions about the code at `offset` in `path`'s buffer, as
/// `routed` picks it, its document's URI and the text the server has.
pub(crate) fn server_at(
    editor: &Editor,
    path: &Path,
    offset: usize,
) -> Option<(Rc<Client>, Uri, Snapshot)> {
    let buffer = (editor.buffers().iter()).find(|buffer| buffer.path() == Some(path))?;
    let list = &buffer.local::<Documents>()?.list;
    let asked = asked_at(editor, buffer, offset);
    let document =
        (list.iter().zip(asked)).find_map(|(document, asked)| asked.then_some(document))?;
    Some((
        Rc::clone(&document.client),
        document.uri.clone(),
        document.sent_text(),
    ))
}

/// Which of `buffer`'s documents a question about the code at `offset` goes to.
fn asked_at(editor: &Editor, buffer: &Buffer, offset: usize) -> Vec<bool> {
    let list = buffer
        .local::<Documents>()
        .map_or(&[][..], |documents| &documents.list);
    let speakers: Vec<_> = (list.iter())
        .map(|document| document.speaks(buffer.mode()))
        .collect();
    routed(&speakers, editor.language_at(buffer, offset), buffer.mode())
}

/// Whether questions about `buffer` wait for its embedded code to be told, which takes
/// milliseconds after opening it or an edit; asked now, they would all go to the file's
/// server.
pub(crate) fn code_untold(editor: &Editor, buffer: &Buffer) -> bool {
    let clients = editor.find_plugin_state::<Clients>();
    clients.is_none_or(|clients| clients.settings().embedded_code())
        && buffer.local::<Documents>().is_some()
        && editor.embedded(buffer).is_none()
}

/// The server that formats `path`'s buffer, its document's URI and the text the server has:
/// the first, unless one of the file's servers formats ahead of it.
fn formatter(editor: &Editor, path: &Path) -> Option<(Rc<Client>, Uri, Snapshot)> {
    let file = remacs_core::embedded_file(path);
    let list = &editor
        .buffers()
        .iter()
        .find(|buffer| buffer.path() == Some(file.as_deref().unwrap_or(path)))?
        .local::<Documents>()?
        .list;
    // Without a server of its own, embedded code is the file's server's, at the same offsets.
    // Embedded code is formatted by regions, which formatters ahead of the server may lack.
    let document = (list.iter())
        .find(|document| file.is_some() && document.path == path)
        .or_else(|| {
            let whole = || list.iter().filter(|document| document.mirror.is_none());
            (whole().find(|document| document.server.formats_first)).or_else(|| whole().next())
        })?;
    Some((
        Rc::clone(&document.client),
        document.uri.clone(),
        document.sent_text(),
    ))
}

/// The text the server of embedded code has of document `path`, whose positions refer
/// to it rather than to the file's text.
pub(crate) fn embedded_text(editor: &Editor, path: &Path) -> Option<Snapshot> {
    let file = remacs_core::embedded_file(path)?;
    let documents = editor
        .buffers()
        .iter()
        .find(|buffer| buffer.path() == Some(file.as_path()))?
        .local::<Documents>()?;
    let document = documents
        .list
        .iter()
        .find(|document| document.path == path)?;
    Some(document.sent_text())
}

/// Hands answers over, keeps every buffer's servers up to date and passes their
/// diagnostics on, then sends what the plugins ask, including what the answers made them
/// ask. Runs after every command and whenever a server published or answered something.
pub(crate) fn sync(editor: &mut Editor, servers: &'static [LanguageServer]) {
    report_exits(editor);
    hand_over_answers(editor);
    // After the answers, so that a file a definition opened is attached at once.
    for index in 0..editor.buffers().len() {
        attach(editor, index, servers);
        let embedded = editor.embedded(&editor.buffers()[index]);
        let buffer = &mut editor.buffers_mut()[index];
        let (text, modified) = (buffer.text().snapshot(), buffer.is_modified());
        let Some(documents) = buffer.local_mut::<Documents>() else {
            continue;
        };
        let reported: Vec<_> = documents
            .list
            .iter_mut()
            .filter_map(|document| {
                let (text, diagnostics) =
                    document.sync(text.clone(), embedded.as_deref(), modified)?;
                Some((document.checker(), text, diagnostics))
            })
            .collect();
        for (checker, text, diagnostics) in reported {
            diagnostics::publish(buffer, checker, text, diagnostics);
        }
    }
    send_requests(editor);
}

fn hand_over_answers(editor: &mut Editor) {
    let Some(documents) = editor.window_buffer_mut().local_mut::<Documents>() else {
        return;
    };
    let mut answered = Vec::new();
    for first in 0..documents.list.len() {
        let client = Rc::clone(&documents.list[first].client);
        // A server holding several of the buffer's documents answers for all at once.
        let earlier = &documents.list[..first];
        if earlier
            .iter()
            .any(|document| Rc::ptr_eq(&document.client, &client))
        {
            continue;
        }
        for mut answer in client.take_answers() {
            let index = documents.answering(first, &answer);
            documents.list[index].keep(&mut answer);
            answered.push((index, Rc::clone(&client), answer));
        }
    }
    for (index, client, answer) in answered {
        hand_over(editor, index, &client, answer);
    }
}

/// Passes on what document `index`'s server answered.
fn hand_over(editor: &mut Editor, index: usize, client: &Client, answer: Answer) {
    let documents = editor.window_buffer_mut().local_mut::<Documents>();
    match answer {
        Answer::Listed { id, list, .. } => {
            let completions = documents.map(|documents| &mut documents.completions);
            if let Some(lists) = completions.and_then(|lists| gather(lists, id, index, list)) {
                let list = List {
                    incomplete: lists.iter().any(|list| list.incomplete),
                    items: lists.into_iter().flat_map(|list| list.items).collect(),
                };
                remacs_completion::deliver(editor, id, list);
            }
        }
        Answer::Resolved {
            id,
            index: item_index,
            item,
        } => {
            let before: usize = documents.map_or(0, |documents| {
                documents.list[..index]
                    .iter()
                    .map(|document| document.listed_count(id))
                    .sum()
            });
            remacs_completion::resolved(editor, id, before + item_index, item);
        }
        Answer::Hover { id, answer } => {
            let hovers = documents.map(|documents| &mut documents.hovers);
            if let Some(answers) = hovers.and_then(|hovers| gather(hovers, id, index, answer)) {
                let answer = answers.into_iter().flatten().next();
                remacs_documentation::deliver(editor, id, answer);
            }
        }
        Answer::Locations {
            id,
            targets,
            encoding,
        } => {
            let locations = definition::locations(editor, targets, encoding);
            remacs_xref::deliver(editor, id, locations);
        }
        Answer::Signatures { id, signatures } => {
            remacs_documentation::deliver_signatures(editor, id, signatures);
        }
        Answer::CodeActions { id, menu, .. } => gather_actions(editor, id, index, menu),
        Answer::ResolvedAction {
            id,
            action,
            encoding,
            settled,
        } => {
            if !settled && action.edit.is_none() && action.data.is_some() {
                let documents = editor.window_buffer_mut().local_mut::<Documents>();
                if let Some(document) =
                    documents.and_then(|documents| documents.list.get_mut(index))
                {
                    document.unresolved = Some((id, action));
                }
                return;
            }
            let action = CodeActionOrCommand::CodeAction(action);
            let result = code_action::carry_out(editor, client, id, action, encoding, false);
            if let Err(error) = result {
                remacs_actions::failed(editor, id, &error.0);
            }
        }
        Answer::ActionFailed { id, message } => remacs_actions::failed(editor, id, &message),
        Answer::Formatted { id, formatted } => remacs_format::deliver(editor, id, formatted),
        Answer::ApplyEdit {
            reply,
            edit,
            encoding,
        } => {
            let result = match client.executing() {
                Some(id) => code_action::apply(editor, id, edit, encoding),
                None => Err("No code action asked for edits".into()),
            };
            let result = match result {
                Ok(()) => json!({ "applied": true }),
                Err(error) => {
                    editor.set_error(error.0.clone());
                    json!({ "applied": false, "failureReason": error.0 })
                }
            };
            client.send(Outgoing::Reply { id: reply, result });
        }
    }
}

/// Delivers the code actions of list `id` once every server offered its own, in the
/// servers' order.
fn gather_actions(editor: &mut Editor, id: u64, index: usize, menu: Vec<Action>) {
    let Some(documents) = editor.window_buffer_mut().local_mut::<Documents>() else {
        return;
    };
    if let Some(menus) = gather(&mut documents.actions, id, index, menu) {
        remacs_actions::deliver(editor, id, menus.into_iter().flatten().collect());
    }
}

/// Sends the window buffer's requests to its servers; without any, questions are answered
/// with nothing at once.
fn send_requests(editor: &mut Editor) {
    send_formats(editor);
    // Left with their plugins, they are taken once the parser tells the code.
    // shortcut: held questions are not tied to their buffer or text, so one followed within
    // the parse by a switch or an edit goes out for what is shown then; tie them if seen.
    if code_untold(editor, editor.window_buffer()) {
        return;
    }
    let completions = remacs_completion::take_requests(editor);
    let hovers = remacs_documentation::take_requests(editor);
    let mut signatures = remacs_documentation::take_signature_requests(editor);
    let definitions = remacs_xref::take_requests(editor);
    let actions = remacs_actions::take_requests(editor);
    let clients = editor.find_plugin_state::<Clients>();
    if clients.is_some_and(|clients| !clients.settings().signature_help_on_typing()) {
        let (typed, kept) = signatures
            .into_iter()
            .partition(|request| matches!(request.trigger, SignatureTrigger::Typed(_)));
        signatures = kept;
        for request in typed {
            remacs_documentation::deliver_signatures(editor, request.id, None);
        }
    }
    let Some(documents) = editor.window_buffer().local::<Documents>() else {
        for request in hovers {
            remacs_documentation::deliver(editor, request.id, None);
        }
        for request in signatures {
            remacs_documentation::deliver_signatures(editor, request.id, None);
        }
        for request in definitions {
            remacs_xref::deliver(editor, request.id, Vec::new());
        }
        for request in actions {
            if let ActionRequest::List { id, .. } = request {
                remacs_actions::deliver(editor, id, Vec::new());
            }
        }
        return;
    };
    let asked = |offset| asked_at(editor, editor.window_buffer(), offset);
    let each = |asked: &[bool]| {
        let list = documents.list.iter().zip(asked.to_vec());
        list.filter_map(|(document, asked)| asked.then_some(document))
    };
    // Asked of no server, so answered with nothing at once.
    let (mut no_completions, mut no_hovers, mut no_definitions, mut no_actions) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut no_signatures = Vec::new();
    let mut completing = None;
    for request in completions {
        match request {
            Request::Complete { id, offset, .. } => {
                let asked = asked(offset);
                if !asked.contains(&true) {
                    no_completions.push(id);
                    continue;
                }
                for document in each(&asked) {
                    document.request(request.clone());
                }
                completing = Some((id, asked));
            }
            Request::Resolve { id, index } => {
                if let Some((document, index)) = documents.listed(id, index) {
                    document.request(Request::Resolve { id, index });
                }
            }
        }
    }
    let mut hovering = None;
    for request in hovers {
        let asked = asked(request.offset);
        if !asked.contains(&true) {
            no_hovers.push(request.id);
            continue;
        }
        for document in each(&asked) {
            document.ask(Question::Hover, request.id, request.offset);
        }
        hovering = Some((request.id, asked));
    }
    for request in signatures {
        // Most keys typed open no call; answered here, they never wake the editor again.
        match each(&asked(request.offset)).next() {
            Some(document) if document.client.may_sign(request.trigger) => {
                document.ask_signatures(request);
            }
            _ => no_signatures.push(request.id),
        }
    }
    for request in definitions {
        let question = match request.lookup {
            Lookup::Definitions => Question::Definition,
            Lookup::References => Question::References,
        };
        match each(&asked(request.offset)).next() {
            Some(document) => document.ask(question, request.id, request.offset),
            None => no_definitions.push(request.id),
        }
    }
    let mut listed = None;
    let mut chosen = Vec::new();
    for request in actions {
        match request {
            ActionRequest::List { id, range, kind } => {
                let asked = asked(range.start);
                if !asked.contains(&true) {
                    no_actions.push(id);
                    continue;
                }
                for document in each(&asked) {
                    document.ask_actions(id, range.clone(), kind);
                }
                listed = Some((id, asked));
            }
            ActionRequest::Apply { id, index } => chosen.push((id, documents.offered(id, index))),
        }
    }
    for id in no_completions {
        remacs_completion::deliver(editor, id, List::default());
    }
    for id in no_hovers {
        remacs_documentation::deliver(editor, id, None);
    }
    for id in no_signatures {
        remacs_documentation::deliver_signatures(editor, id, None);
    }
    for id in no_definitions {
        remacs_xref::deliver(editor, id, Vec::new());
    }
    for id in no_actions {
        remacs_actions::deliver(editor, id, Vec::new());
    }
    let documents = editor
        .window_buffer_mut()
        .local_mut::<Documents>()
        .expect("the buffer has documents");
    if let Some((id, asked)) = completing {
        documents.completions = Gathering::new(id, &asked);
    }
    if let Some((id, asked)) = hovering {
        documents.hovers = Gathering::new(id, &asked);
    }
    if let Some((id, asked)) = listed {
        documents.actions = Gathering::new(id, &asked);
    }
    for (id, offered) in chosen {
        let result = match offered {
            Some((client, action, encoding)) => {
                code_action::carry_out(editor, &client, id, action, encoding, true)
            }
            None => Err("The server no longer offers it".into()),
        };
        if let Err(error) = result {
            remacs_actions::failed(editor, id, &error.0);
        }
    }
}

/// Format requests held back while their server loads the project: rust-analyzer then
/// formats without the crate's edition, so rustfmt sorts and wraps in an older style.
#[derive(Default)]
struct HeldFormats(Vec<FormatRequest>);

/// Sends each format request, held back or new, to the first server of the buffer
/// visiting its file once that server has loaded the project.
fn send_formats(editor: &mut Editor) {
    let mut requests = mem::take(&mut editor.plugin_state::<HeldFormats>().0);
    let held = requests.len();
    // The last backend: what the others left is the language server's to format.
    requests.extend(remacs_format::take_requests(editor, |_| true));
    for (index, request) in requests.into_iter().enumerate() {
        let Some((client, uri, text)) = formatter(editor, &request.path) else {
            let missing = Failure::Unsupported("No formatter for this buffer".into());
            remacs_format::deliver(editor, request.id, Err(missing));
            continue;
        };
        if !client.is_settled() {
            if index >= held {
                editor.set_message("Waiting for the language server to load the project…");
            }
            editor.plugin_state::<HeldFormats>().0.push(request);
            continue;
        }
        client.send(Outgoing::Format {
            uri,
            id: request.id,
            text,
            range: request.range,
        });
    }
}

/// The servers for `mode` that may serve `path`, in the servers' order: of a group of
/// alternatives, only the first not known to pass it over.
pub(crate) fn wanted(
    servers: &'static [LanguageServer],
    clients: Option<&Clients>,
    mode: &str,
    path: &Path,
) -> Vec<(&'static LanguageServer, &'static str)> {
    let mut groups = HashSet::new();
    servers
        .iter()
        .filter_map(|server| Some((server, server.language_id(mode)?)))
        .filter(|(server, _)| !clients.is_some_and(|clients| clients.passes_over(server, path)))
        .filter(|(server, _)| server.group.is_none_or(|group| groups.insert(group)))
        .collect()
}

/// A server a buffer's file, or the code of one language embedded in it, is wanted on.
struct Want {
    server: &'static LanguageServer,
    language: &'static str,
    /// The file's, or that of a document of its embedded code.
    path: PathBuf,
    embedded: Option<Embedded>,
}

/// The servers wanted for buffer `index`: those of its mode for its file, then those of
/// each embedded language for a document of that code.
fn wants(editor: &Editor, index: usize, servers: &'static [LanguageServer]) -> Vec<Want> {
    let buffer = &editor.buffers()[index];
    let Some(path) = buffer
        .path()
        .filter(|_| buffer.text().len() <= MAX_SYNCED_BYTES)
    else {
        return Vec::new();
    };
    let clients = editor.find_plugin_state::<Clients>();
    let mut wants: Vec<Want> = wanted(servers, clients, buffer.mode(), path)
        .into_iter()
        .map(|(server, language)| Want {
            server,
            language,
            path: path.to_path_buf(),
            embedded: None,
        })
        .collect();
    if clients.is_some_and(|clients| !clients.settings().embedded_code()) {
        return wants;
    }
    let Some(embedded) = editor.embedded(buffer) else {
        // Until its code can be told again, the documents of the code it had stay.
        let attached = buffer.local::<Documents>().map(|documents| &documents.list);
        wants.extend(attached.into_iter().flatten().filter_map(|document| {
            let mirror = document.mirror.as_ref()?;
            Some(Want {
                server: document.server,
                language: document.language,
                path: document.path.clone(),
                embedded: Some(Embedded {
                    mode: mirror.mode,
                    ranges: mirror.ranges.clone(),
                }),
            })
        }));
        return wants;
    };
    for code in embedded {
        let document = editor.embedded_document(path, code.mode);
        for (server, language) in wanted(servers, clients, code.mode, &document) {
            wants.push(Want {
                server,
                language,
                path: document.clone(),
                embedded: Some(code.clone()),
            });
        }
    }
    wants
}

/// Opens the buffer's file on every server for its mode that could start, in the servers'
/// order, and its embedded code on the servers of its languages; closes those no longer
/// wanted. A group's server found missing makes way for the next of the group at once.
fn attach(editor: &mut Editor, index: usize, servers: &'static [LanguageServer]) {
    while attach_once(editor, index, servers) {}
}

/// `attach`; true when a server of a group was found missing.
fn attach_once(editor: &mut Editor, index: usize, servers: &'static [LanguageServer]) -> bool {
    let wanted = wants(editor, index, servers);
    let attached = editor.buffers()[index]
        .local::<Documents>()
        .map_or(&[][..], |documents| &documents.list);
    if attached.len() == wanted.len()
        && attached
            .iter()
            .zip(&wanted)
            .all(|(document, want)| document.serves(want))
    {
        return false;
    }
    let buffer = &mut editor.buffers_mut()[index];
    let (mut kept, closed): (Vec<_>, Vec<_>) = buffer
        .local_mut::<Documents>()
        .map(mem::take)
        .unwrap_or_default()
        .list
        .into_iter()
        .partition(|document| wanted.iter().any(|want| document.serves(want)));
    for document in closed {
        diagnostics::clear(buffer, document.checker());
    }
    let mut list = Vec::new();
    let mut next_of_group = false;
    for want in wanted {
        if let Some(position) = kept.iter().position(|document| document.serves(&want)) {
            list.push(kept.remove(position));
            continue;
        }
        match open(editor, index, &want) {
            Some(document) => list.push(document),
            None => {
                next_of_group |= want.server.group.is_some()
                    && editor
                        .plugin_state::<Clients>()
                        .passes_over(want.server, &want.path);
            }
        }
    }
    let buffer = &mut editor.buffers_mut()[index];
    if list.is_empty() {
        buffer.remove_local::<Documents>();
    } else {
        buffer.set_local(Documents {
            list,
            ..Documents::default()
        });
    }
    next_of_group
}

/// Opens buffer `index`'s file, or its embedded code, on the server `want` names,
/// starting it for the file's project first.
fn open(editor: &mut Editor, index: usize, want: &Want) -> Option<Document> {
    let server = want.server;
    let notifier = editor.notifier();
    let downloaded = server
        .tool
        .and_then(|tool| remacs_tools::installed(editor, tool.name));
    let client = match editor
        .plugin_state::<Clients>()
        .client_for(server, &want.path, downloaded, notifier)
    {
        Ok(client) => client?,
        Err(NotStarted::Missing) => {
            let mode = editor.buffers()[index].mode();
            let mode = want.embedded.as_ref().map_or(mode, |code| code.mode);
            offer::missing(editor, server, &want.path, mode);
            return None;
        }
        Err(NotStarted::Failed(message)) => {
            editor.set_error(message);
            return None;
        }
    };
    let buffer = &mut editor.buffers_mut()[index];
    let text = buffer.text().snapshot();
    let modes = CodeModes {
        document: want
            .embedded
            .as_ref()
            .map_or(buffer.mode(), |code| code.mode),
        languages: server.languages,
    };
    let document = Document::open(want, client, modes, text.clone(), buffer.is_modified());
    diagnostics::publish(buffer, document.checker(), text, Vec::new());
    Some(document)
}

/// The outermost directory with a root marker inside the Git work tree; else the work
/// tree, else the file's directory; never the home directory or above. One server then
/// covers a whole Cargo workspace or monorepo.
pub(crate) fn project_root(path: &Path, markers: &[&str]) -> PathBuf {
    let directory = path.parent().unwrap_or(path);
    let mut outermost = None;
    for ancestor in project_ancestors(directory) {
        if markers.iter().any(|marker| has_file(ancestor, marker)) {
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
    fn answers_are_gathered_from_every_server_in_their_order() {
        let mut gathering = Gathering::new(2, &[true; 3]);
        assert_eq!(gather(&mut gathering, 1, 0, "old"), None);
        assert_eq!(gather(&mut gathering, 2, 2, "tailwind"), None);
        assert_eq!(gather(&mut gathering, 2, 0, "typescript"), None);
        assert_eq!(
            gather(&mut gathering, 2, 1, "eslint"),
            Some(vec!["typescript", "eslint", "tailwind"])
        );
        assert!(gathering.is_none());
        assert_eq!(gather(&mut gathering, 2, 1, "late"), None);

        // Servers of another language than the one asked about answered nothing.
        let mut gathering = Gathering::new(3, &[false, true]);
        assert_eq!(gather(&mut gathering, 3, 1, "css"), Some(vec!["", "css"]));
    }

    #[test]
    fn a_program_is_found_in_the_node_modules_of_a_package_it_comes_with() {
        let root = std::env::temp_dir().join(format!("remacs-node-program-{}", std::process::id()));
        let bin = root.join("node_modules/vite-plus/node_modules/.bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("oxlint"), "").unwrap();
        let server = |dependencies: &'static [&'static str]| LanguageServer {
            command: "oxlint",
            when: crate::catalog::When {
                dependencies,
                ..Default::default()
            },
            ..LanguageServer::default()
        };

        let modules = root.join("node_modules");
        let program = node_program(&modules, &server(&["oxlint", "vite-plus"])).unwrap();
        assert_eq!(program.path, bin.join("oxlint"));
        let (name, value) = &program.env[0];
        assert_eq!(name, "PATH");
        assert!(value.starts_with(&format!("{}:", bin.display())));
        assert!(node_program(&modules, &server(&["oxlint"])).is_none());

        std::fs::create_dir_all(root.join("node_modules/.bin")).unwrap();
        std::fs::write(root.join("node_modules/.bin/oxlint"), "").unwrap();
        let program = node_program(&modules, &server(&["vite-plus"])).unwrap();
        assert_eq!(program.path, root.join("node_modules/.bin/oxlint"));

        // A `:` cannot go on `PATH`; the project's program still runs.
        let colon = root.join("a:b/node_modules/.bin");
        std::fs::create_dir_all(&colon).unwrap();
        std::fs::write(colon.join("oxlint"), "").unwrap();
        let program = node_program(&root.join("a:b/node_modules"), &server(&[])).unwrap();
        assert_eq!((program.path, program.env), (colon.join("oxlint"), vec![]));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn questions_go_to_the_code_s_servers_else_the_file_s() {
        let page = ["HTML", "HTML", "JavaScript"];
        assert_eq!(routed(&page, "JavaScript", "HTML"), [false, false, true]);
        assert_eq!(routed(&page, "HTML", "HTML"), [true, true, false]);
        // No CSS server: the HTML server answers for it.
        assert_eq!(routed(&page, "CSS", "HTML"), [true, true, false]);
        // No HTML server: nothing is asked.
        assert_eq!(routed(&["JavaScript"], "HTML", "HTML"), [false]);
    }

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
