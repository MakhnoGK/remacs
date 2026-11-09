//! Servers hearing of their projects' files changing on disk (ADR 0096): a server is told
//! of the files it registered for (`workspace/didChangeWatchedFiles`), and restarted when
//! one of its `config-files` it did not register for changes. One watch per project root,
//! on a thread that sleeps until a file changes.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use std::{fs, mem, thread};

use globset::{GlobBuilder, GlobMatcher};
use lsp_types::notification::{DidChangeWatchedFiles, Notification};
use lsp_types::{
    DidChangeWatchedFilesRegistrationOptions, FileChangeType, FileEvent, GlobPattern, OneOf,
    WatchKind,
};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use remacs_core::{Editor, Notifier};
use serde_json::Value;

use crate::LanguageServer;
use crate::client::{Client, Outgoing, SharedReplies, send_reply};
use crate::document::{self, Clients};
use crate::uri;

/// Changes that never quiet down, as a build writing files, are still heard after this
/// many delays.
const MAX_DELAYS: u32 = 5;

pub(crate) fn lock<T>(shared: &Mutex<T>) -> MutexGuard<'_, T> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The globs a server registered, by registration id, each with the changes it wants.
#[derive(Default)]
pub(crate) struct Registered(Vec<(String, Vec<(GlobMatcher, WatchKind)>)>);

pub(crate) type SharedRegistered = Arc<Mutex<Registered>>;

impl Registered {
    /// Keeps the file watchers `client/registerCapability`'s `params` register.
    pub(crate) fn register(&mut self, params: &Value) {
        let registrations = params["registrations"].as_array().into_iter().flatten();
        for registration in registrations.filter(|it| it["method"] == DidChangeWatchedFiles::METHOD)
        {
            let options = registration["registerOptions"].clone();
            let Ok(options) =
                serde_json::from_value::<DidChangeWatchedFilesRegistrationOptions>(options)
            else {
                continue;
            };
            let globs = (options.watchers.into_iter())
                .filter_map(|watcher| {
                    let pattern = match watcher.glob_pattern {
                        GlobPattern::String(pattern) => pattern,
                        GlobPattern::Relative(relative) => {
                            let base = match &relative.base_uri {
                                OneOf::Left(folder) => &folder.uri,
                                OneOf::Right(uri) => uri,
                            };
                            let base = uri::to_path(base)?;
                            let base = globset::escape(&base.to_string_lossy());
                            format!("{base}/{}", relative.pattern)
                        }
                    };
                    Some((glob(&pattern)?, watcher.kind.unwrap_or(WatchKind::all())))
                })
                .collect();
            let id = registration["id"].as_str().unwrap_or_default().to_owned();
            self.0.push((id, globs));
        }
    }

    /// Drops the registrations `client/unregisterCapability`'s `params` name.
    pub(crate) fn unregister(&mut self, params: &Value) {
        // Misspelt so in the protocol.
        for gone in params["unregisterations"].as_array().into_iter().flatten() {
            self.0.retain(|(id, _)| gone["id"] != *id);
        }
    }

    fn wants(&self, path: &Path, kind: WatchKind) -> bool {
        (self.0.iter().flat_map(|(_, globs)| globs))
            .any(|(glob, kinds)| kinds.contains(kind) && glob.is_match(path))
    }
}

/// `*` within a name, `**` across directories, as LSP globs go.
pub(crate) fn glob(pattern: &str) -> Option<GlobMatcher> {
    let glob = GlobBuilder::new(pattern).literal_separator(true).build().ok()?;
    Some(glob.compile_matcher())
}

/// A running server as the watch's thread sees it.
struct Route {
    name: &'static str,
    root: PathBuf,
    /// The root as the system names it, as some report changes, e.g. macOS under
    /// `/private`.
    canonical: PathBuf,
    /// Relative to the root.
    config: Vec<GlobMatcher>,
    registered: SharedRegistered,
    replies: SharedReplies,
}

/// Between the editor and the watch's thread.
#[derive(Default)]
struct Shared {
    routes: Vec<Route>,
    delay: Duration,
    /// Servers to restart, by name and root, with the file that changed; until the editor
    /// takes them.
    restarts: HashMap<(&'static str, PathBuf), PathBuf>,
}

/// Watches the project roots of the running servers.
#[derive(Default)]
pub(crate) struct ProjectWatch {
    shared: Arc<Mutex<Shared>>,
    /// None while no server runs, which ends its thread.
    watcher: Option<RecommendedWatcher>,
    roots: HashSet<PathBuf>,
    /// The clients routed to, to tell when those running change.
    clients: Vec<Weak<Client>>,
}

/// Post-command hook: restarts the servers whose config files changed, and watches the
/// roots of those running.
pub(crate) fn sync(editor: &mut Editor, servers: &[LanguageServer]) {
    let delay = editor.plugin_state::<Clients>().settings().watch_delay();
    let restarts = {
        let mut shared = lock(&editor.plugin_state::<ProjectWatch>().shared);
        shared.delay = delay;
        mem::take(&mut shared.restarts)
    };
    for ((name, root), file) in &restarts {
        document::restart_at(editor, name, root);
        let file = file.strip_prefix(root).unwrap_or(file).display();
        editor.set_done(format!("Restarted {name}: {file} changed"));
    }
    if !restarts.is_empty() {
        // The next sync starts them again.
        editor.notifier()();
    }
    follow(editor, servers);
}

/// Routes changes to the servers running now, if they changed since the last call.
fn follow(editor: &mut Editor, servers: &[LanguageServer]) {
    let notifier = editor.notifier();
    let mut watch = mem::take(editor.plugin_state::<ProjectWatch>());
    let clients = editor.plugin_state::<Clients>();
    let unchanged = clients.running_servers().count() == watch.clients.len()
        && clients.running_servers().all(|(_, _, client)| {
            watch.clients.iter().any(|known| known.as_ptr() == Rc::as_ptr(client))
        });
    if unchanged {
        *editor.plugin_state::<ProjectWatch>() = watch;
        return;
    }
    let routes: Vec<Route> = clients
        .running_servers()
        .map(|(name, root, client)| {
            let server = servers.iter().find(|server| server.name == name);
            let (registered, replies) = client.listener();
            Route {
                name,
                root: root.to_path_buf(),
                canonical: fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()),
                config: server
                    .map_or(&[][..], |server| server.config_files)
                    .iter()
                    .filter_map(|pattern| glob(pattern))
                    .collect(),
                registered,
                replies,
            }
        })
        .collect();
    watch.clients = clients.running_servers().map(|(_, _, client)| Rc::downgrade(client)).collect();
    // shortcut: only running servers' roots are watched, so a first config file where its
    // server was declined (`when`) starts nothing until `lsp-restart`; watch those too if
    // that is missed.
    let roots: HashSet<PathBuf> = routes.iter().map(|route| route.root.clone()).collect();
    if roots.is_empty() {
        watch.watcher = None;
    } else if watch.watcher.is_none() {
        watch.roots.clear();
        match start(&watch.shared, notifier) {
            Ok(watcher) => watch.watcher = Some(watcher),
            Err(error) => editor.set_error(format!("Watching projects for servers: {error}")),
        }
    }
    if let Some(watcher) = &mut watch.watcher {
        let mut paths = watcher.paths_mut();
        for gone in watch.roots.difference(&roots) {
            let _ = paths.remove(gone);
        }
        // A root gone meanwhile stays unwatched; its servers have nothing left to hear of.
        for added in roots.difference(&watch.roots) {
            let _ = paths.add(added, RecursiveMode::Recursive);
        }
        let _ = paths.commit();
    }
    watch.roots = roots;
    lock(&watch.shared).routes = routes;
    *editor.plugin_state::<ProjectWatch>() = watch;
}

fn start(shared: &Arc<Mutex<Shared>>, notifier: Notifier) -> notify::Result<RecommendedWatcher> {
    let (events, received) = mpsc::channel();
    let watcher = notify::recommended_watcher(move |event| {
        let _ = events.send(event);
    })?;
    let shared = Arc::clone(shared);
    thread::Builder::new()
        .name("lsp-watch".into())
        .spawn(move || listen(&received, &shared, &notifier))?;
    Ok(watcher)
}

/// Gathers changes until they quiet down, then routes them; ends with the watcher.
fn listen(received: &Receiver<notify::Result<Event>>, shared: &Mutex<Shared>, notifier: &Notifier) {
    while let Ok(first) = received.recv() {
        let mut changed = HashMap::new();
        gather(&mut changed, first);
        let delay = lock(shared).delay;
        let deadline = Instant::now() + delay * MAX_DELAYS;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match received.recv_timeout(delay.min(left)) {
                Ok(event) => gather(&mut changed, event),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        if route(&mut lock(shared), changed) {
            notifier();
        }
    }
}

/// Notes the paths `event` changed, each with whether it was created.
fn gather(changed: &mut HashMap<PathBuf, bool>, event: notify::Result<Event>) {
    let Ok(event) = event else { return };
    if matches!(event.kind, EventKind::Access(_)) {
        return;
    }
    let created = matches!(event.kind, EventKind::Create(_));
    for path in event.paths {
        *changed.entry(path).or_default() |= created;
    }
}

/// Tells each server of the changed files it registered for, and notes for a restart
/// those whose other changed files include a config file; true when one is newly noted.
fn route(shared: &mut Shared, changed: HashMap<PathBuf, bool>) -> bool {
    let quiet = shared.restarts.is_empty();
    let changed: Vec<_> = (changed.into_iter())
        .map(|(path, created)| {
            let kind = match (path.exists(), created) {
                (false, _) => (WatchKind::Delete, FileChangeType::DELETED),
                (true, true) => (WatchKind::Create, FileChangeType::CREATED),
                (true, false) => (WatchKind::Change, FileChangeType::CHANGED),
            };
            (path, kind)
        })
        .collect();
    for route in &shared.routes {
        let mut told = Vec::new();
        for (path, (kind, change)) in &changed {
            let Ok(relative) =
                path.strip_prefix(&route.canonical).or_else(|_| path.strip_prefix(&route.root))
            else {
                continue;
            };
            let path = route.root.join(relative);
            if lock(&route.registered).wants(&path, *kind) {
                told.push(FileEvent::new(uri::from_path(&path), *change));
            } else if route.config.iter().any(|glob| glob.is_match(relative)) {
                shared.restarts.insert((route.name, route.root.clone()), path);
            }
        }
        if !told.is_empty() {
            send_reply(&route.replies, Outgoing::WatchedFiles { changes: told });
        }
    }
    quiet && !shared.restarts.is_empty()
}

#[cfg(test)]
mod tests {
    use remacs_core::Buffer;
    use remacs_text::Text;
    use serde_json::json;

    use super::*;
    use crate::LspPlugin;

    fn temporary_root(test: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("remacs-watch-{test}-{}", std::process::id()));
        fs::create_dir_all(root.join("src")).unwrap();
        root
    }

    fn registered(root: &Path) -> Registered {
        let mut registered = Registered::default();
        registered.register(&json!({ "registrations": [
            { "id": "rs", "method": "workspace/didChangeWatchedFiles", "registerOptions": {
                "watchers": [{ "globPattern": format!("{}/**/*.rs", root.display()) }] } },
            { "id": "toml", "method": "workspace/didChangeWatchedFiles", "registerOptions": {
                "watchers": [{
                    "globPattern": { "baseUri": uri::from_path(root), "pattern": "**/Cargo.toml" },
                    "kind": 2,
                }] } },
            { "id": "other", "method": "textDocument/formatting" },
        ] }));
        registered
    }

    #[test]
    fn registered_globs_pick_files_and_changes() {
        let root = Path::new("/project");
        let mut registered = registered(root);
        let wants =
            |registered: &Registered, file: &str, kind| registered.wants(&root.join(file), kind);
        assert!(wants(&registered, "src/main.rs", WatchKind::Create));
        assert!(wants(&registered, "lib.rs", WatchKind::Delete));
        assert!(wants(&registered, "app/Cargo.toml", WatchKind::Change));
        assert!(!wants(&registered, "app/Cargo.toml", WatchKind::Delete), "only changes");
        assert!(!wants(&registered, "rust-analyzer.toml", WatchKind::Change));
        registered.unregister(&json!({ "unregisterations": [{ "id": "rs", "method": "" }] }));
        assert!(!wants(&registered, "src/main.rs", WatchKind::Change));
        assert!(wants(&registered, "Cargo.toml", WatchKind::Change));
    }

    #[test]
    fn config_globs_take_alternatives_and_stay_in_their_directory() {
        let matches = |pattern, path| glob(pattern).unwrap().is_match(Path::new(path));
        let ruff = "{ruff,.ruff}.toml";
        assert!(matches(ruff, "ruff.toml") && matches(ruff, ".ruff.toml"));
        assert!(matches("**/go.{mod,work}", "go.mod") && matches("**/go.{mod,work}", "a/go.work"));
        assert!(!matches("tsconfig*.json", "packages/app/tsconfig.json"));
    }

    /// A route for `root` restarting on `rust-analyzer.toml`, and what it is told.
    fn route_for(root: &Path) -> (Route, Receiver<Outgoing>) {
        let (sender, told) = mpsc::channel();
        let route = Route {
            name: "rust-analyzer",
            root: root.to_path_buf(),
            canonical: fs::canonicalize(root).unwrap(),
            config: vec![glob("rust-analyzer.toml").unwrap(), glob("**/Cargo.toml").unwrap()],
            registered: Arc::new(Mutex::new(registered(root))),
            replies: Arc::new(Mutex::new(Some(sender))),
        };
        (route, told)
    }

    #[test]
    fn registered_files_are_told_of_and_other_config_files_restart() {
        let root = temporary_root("route");
        fs::write(root.join("src/main.rs"), "").unwrap();
        fs::write(root.join("rust-analyzer.toml"), "").unwrap();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        let (server, told) = route_for(&root);
        let mut shared = Shared { routes: vec![server], ..Shared::default() };
        let changed = HashMap::from([
            (root.join("src/main.rs"), true),
            (root.join("gone.rs"), false),
            (root.join("Cargo.toml"), false),
            (root.join("README.md"), false),
            (PathBuf::from("/elsewhere/rust-analyzer.toml"), false),
        ]);
        assert!(!route(&mut shared, changed), "nothing to restart");
        let Ok(Outgoing::WatchedFiles { mut changes }) = told.try_recv() else {
            panic!("the server is told");
        };
        changes.sort_by_key(|event| event.uri.to_string());
        let event = |file: &str, change| FileEvent::new(uri::from_path(&root.join(file)), change);
        let expected = [
            event("Cargo.toml", FileChangeType::CHANGED),
            event("gone.rs", FileChangeType::DELETED),
            event("src/main.rs", FileChangeType::CREATED),
        ];
        assert_eq!(changes, expected);

        // Cargo.toml was registered for changes only.
        fs::remove_file(root.join("Cargo.toml")).unwrap();
        let changed = HashMap::from([
            (root.join("rust-analyzer.toml"), false),
            (root.join("Cargo.toml"), false),
        ]);
        assert!(route(&mut shared, changed));
        let restarted = shared.restarts.keys().cloned().collect::<Vec<_>>();
        assert_eq!(restarted, [("rust-analyzer", root.clone())]);
        assert!(told.try_recv().is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_changed_config_file_on_disk_wakes_the_editor_to_restart() {
        let root = temporary_root("disk");
        let (sender, notified) = mpsc::channel();
        let notifier: Notifier = Arc::new(move || {
            let _ = sender.send(());
        });
        let (server, _told) = route_for(&root);
        let shared = Arc::new(Mutex::new(Shared { routes: vec![server], ..Shared::default() }));
        let mut watcher = start(&shared, notifier).unwrap();
        watcher.watch(&root, RecursiveMode::Recursive).unwrap();
        fs::write(root.join("rust-analyzer.toml"), "[check]\n").unwrap();
        notified.recv_timeout(Duration::from_secs(5)).unwrap();
        let restarts = mem::take(&mut lock(&shared).restarts);
        assert_eq!(restarts.into_keys().collect::<Vec<_>>(), [("rust-analyzer", root.clone())]);
        fs::remove_dir_all(&root).unwrap();
    }

    static PLUGIN: LspPlugin = LspPlugin::new(
        "[[servers]]\n\
         name = \"waiting\"\n\
         command = \"sh\"\n\
         args = [\"-c\", \"cat > /dev/null\"]\n\
         languages = { Fundamental = \"plaintext\" }\n\
         config-files = [\"waiting.toml\"]\n",
    );

    #[test]
    fn a_server_restarts_for_its_project_when_its_config_file_changes() {
        let (sender, notified) = mpsc::channel();
        let mut editor = Editor::new(&[&PLUGIN]);
        editor.set_notifier(Arc::new(move || {
            let _ = sender.send(());
        }));
        let root = temporary_root("restart");
        let file = root.join("notes.txt");
        editor.add_buffer(Buffer::new("notes.txt", Text::default(), "Fundamental").with_file(file));
        editor.refresh();
        let running = |editor: &mut Editor| {
            let clients = editor.plugin_state::<Clients>();
            clients.running().map(|(_, client)| Rc::downgrade(client)).next().unwrap()
        };
        let first = running(&mut editor);
        fs::write(root.join("waiting.toml"), "").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while editor.echo().as_deref() != Some("Restarted waiting: waiting.toml changed") {
            notified.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap();
            editor.refresh();
        }
        editor.refresh();
        assert!(first.upgrade().is_none(), "the old server is gone");
        assert!(running(&mut editor).upgrade().is_some(), "a new one runs");
        fs::remove_dir_all(&root).unwrap();
    }
}
