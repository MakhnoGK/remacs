use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecursiveMode, Watcher};
use remacs_core::Notifier;

use crate::command::{git, output};
use crate::diff::parse_diff;
use crate::history::{HistoryCache, HistoryWants};
use crate::ignore_rules::IgnoreRules;
use crate::settings::GitSettings;
use crate::status::{Counted, DiffKey, Snapshot, parse_status};
use crate::{conflict, details, moves, operation, untracked};

/// Changes arriving this close together cause one read.
const SETTLE_TIME: Duration = Duration::from_millis(100);
/// A tree that never stops changing is still read this often.
const MAX_DELAY: Duration = Duration::from_secs(1);
/// Larger files with conflicts are listed, not read for their markers.
const MAX_CONFLICT_FILE: u64 = 8 * 1024 * 1024;
/// What Git rewrites under its directory without the status changing.
const QUIET_GIT_PATHS: &[&str] = &["objects", "logs"];

/// Reads a working tree's status on a background thread, and again whenever the tree or
/// its repository changes; also runs what changes the index, one thing at a time.
pub struct Monitor {
    messages: Sender<Message>,
    updates: Receiver<Update>,
}

enum Message {
    Changed(notify::Result<Event>),
    /// Relative to the root, from and to.
    RecordMoves(Vec<(PathBuf, PathBuf)>),
    Want(Wants),
    /// With what to say once it is done.
    Run(Job, Option<String>),
    Refresh,
    Stop,
}

pub enum Update {
    Status(Box<Snapshot>),
    Failed(String),
    /// A job is done, as it says.
    Done(String),
}

/// What is read besides the status, for the status buffers showing the tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wants {
    pub details: bool,
    /// An untracked directory's is the list of its files.
    pub diffs: Vec<DiffKey>,
    pub history: Option<HistoryWants>,
    pub settings: GitSettings,
}

/// Changes the repository, run in its root; the error is said to the user.
pub type Job = Box<dyn FnOnce(&Path) -> Result<(), String> + Send>;

impl Monitor {
    pub fn start(root: PathBuf, wants: Wants, notifier: Notifier) -> Self {
        let (messages, received) = mpsc::channel();
        let (send_update, updates) = mpsc::channel();
        let events = messages.clone();
        thread::Builder::new()
            .name("git-monitor".into())
            .spawn(move || {
                let worker = Worker {
                    root,
                    updates: send_update,
                    notifier,
                };
                worker.run(events, &received, wants);
            })
            .expect("spawning the git monitor thread");
        Self { messages, updates }
    }

    pub fn record_moves(&self, moves: Vec<(PathBuf, PathBuf)>) {
        let _ = self.messages.send(Message::RecordMoves(moves));
    }

    /// Reads `wants` from now on, starting at once.
    pub fn want(&self, wants: Wants) {
        let _ = self.messages.send(Message::Want(wants));
    }

    /// Runs `job` after what is already queued, then reads the status again.
    pub fn run(&self, job: Job) {
        let _ = self.messages.send(Message::Run(job, None));
    }

    /// Like [`Self::run`], saying `done` once it succeeded.
    pub fn run_saying(&self, job: Job, done: String) {
        let _ = self.messages.send(Message::Run(job, Some(done)));
    }

    pub fn refresh(&self) {
        let _ = self.messages.send(Message::Refresh);
    }

    /// What happened since the last call, oldest first.
    pub fn updates(&self) -> impl Iterator<Item = Update> + '_ {
        self.updates.try_iter()
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.messages.send(Message::Stop);
    }
}

struct Worker {
    root: PathBuf,
    updates: Sender<Update>,
    notifier: Notifier,
}

impl Worker {
    fn run(&self, events: Sender<Message>, messages: &Receiver<Message>, mut wants: Wants) {
        let git_directories = match git_directories(&self.root) {
            Ok(directories) => directories,
            Err(error) => return self.send(Update::Failed(format!("Git: {error}"))),
        };
        // Watching before the first read keeps changes made during it.
        let _watcher = notify::recommended_watcher(move |event| {
            let _ = events.send(Message::Changed(event));
        })
        .and_then(|mut watcher| {
            watcher.watch(&self.root, RecursiveMode::Recursive)?;
            for directory in git_directories
                .iter()
                .filter(|directory| !directory.starts_with(&self.root))
            {
                watcher.watch(directory, RecursiveMode::Recursive)?;
            }
            Ok(watcher)
        });
        let mut filter = EventFilter {
            root: &self.root,
            rules: IgnoreRules::new(&self.root, &git_directories[0]),
            git_directories: &git_directories,
        };
        let mut reported_failure = false;
        let mut history = HistoryCache::default();
        loop {
            match read(&self.root, &git_directories[0], &wants, &mut history) {
                Ok(snapshot) => {
                    reported_failure = false;
                    self.send(Update::Status(Box::new(snapshot)));
                }
                Err(error) if !reported_failure => {
                    reported_failure = true;
                    self.send(Update::Failed(format!("Git status: {error}")));
                }
                Err(_) => {}
            }
            if !self.wait_for_change(&mut filter, &mut wants, messages) {
                return;
            }
        }
    }

    /// Waits until something that may change the status happened and things settled,
    /// or a read is asked for; false once the monitor is stopped.
    fn wait_for_change(
        &self,
        filter: &mut EventFilter,
        wants: &mut Wants,
        messages: &Receiver<Message>,
    ) -> bool {
        let mut deadline = None;
        loop {
            let message = match deadline {
                None => messages.recv().map_err(|_| RecvTimeoutError::Disconnected),
                Some(deadline) => {
                    let settle = SETTLE_TIME.min(deadline - Instant::now().min(deadline));
                    messages.recv_timeout(settle)
                }
            };
            match message {
                Ok(Message::Changed(event)) => {
                    if filter.matters(event) {
                        deadline.get_or_insert_with(|| Instant::now() + MAX_DELAY);
                    }
                }
                Ok(Message::RecordMoves(moves)) => {
                    if let Err(error) = moves::record(&self.root, &moves) {
                        self.send(Update::Failed(format!(
                            "Could not tell Git of the moves: {error}"
                        )));
                    }
                    deadline.get_or_insert_with(|| Instant::now() + MAX_DELAY);
                }
                Ok(Message::Want(wanted)) => {
                    *wants = wanted;
                    return true;
                }
                Ok(Message::Run(job, done)) => {
                    match job(&self.root) {
                        Err(error) => self.send(Update::Failed(error)),
                        Ok(()) => {
                            if let Some(done) = done {
                                self.send(Update::Done(done));
                            }
                        }
                    }
                    return true;
                }
                Ok(Message::Refresh) | Err(RecvTimeoutError::Timeout) => return true,
                Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    fn send(&self, update: Update) {
        if self.updates.send(update).is_ok() {
            (self.notifier)();
        }
    }
}

/// Decides which file system events may change the status.
struct EventFilter<'a> {
    root: &'a Path,
    rules: IgnoreRules,
    git_directories: &'a [PathBuf],
}

impl EventFilter<'_> {
    fn matters(&mut self, event: notify::Result<Event>) -> bool {
        let Ok(event) = event else {
            return true;
        };
        if event.need_rescan() {
            return true;
        }
        if matches!(event.kind, EventKind::Access(_)) {
            return false;
        }
        // Every path is looked at, so that each changed ignore file is forgotten.
        event
            .paths
            .iter()
            .fold(false, |matters, path| self.path_matters(path) | matters)
    }

    fn path_matters(&mut self, path: &Path) -> bool {
        if let Some(inside) = self
            .git_directories
            .iter()
            .find_map(|directory| path.strip_prefix(directory).ok())
        {
            let quiet = inside.components().next().is_some_and(|first| {
                QUIET_GIT_PATHS.contains(&&*first.as_os_str().to_string_lossy())
            });
            return !quiet && path.extension().is_none_or(|extension| extension != "lock");
        }
        if !path.starts_with(self.root) {
            return false;
        }
        if path.file_name().is_some_and(|name| name == ".gitignore") {
            if let Some(directory) = path.parent() {
                self.rules.forget(directory);
            }
            return true;
        }
        !self.rules.is_ignored(path)
    }
}

/// The repository's directory and, for a linked working tree, the main one's.
fn git_directories(root: &Path) -> io::Result<Vec<PathBuf>> {
    let printed = output(git(root).args([
        "rev-parse",
        "--path-format=absolute",
        "--git-dir",
        "--git-common-dir",
    ]))?;
    let mut directories: Vec<PathBuf> = String::from_utf8_lossy(&printed)
        .lines()
        .map(PathBuf::from)
        .collect();
    directories.dedup();
    if directories.is_empty() {
        return Err(io::Error::other("no repository"));
    }
    Ok(directories)
}

fn read(
    root: &Path,
    git_directory: &Path,
    wants: &Wants,
    history: &mut HistoryCache,
) -> io::Result<Snapshot> {
    let mut snapshot = parse_status(&output(git(root).args([
        "status",
        "--porcelain=v2",
        "-z",
        "--branch",
        "--show-stash",
    ]))?);
    if !snapshot.files.is_empty() {
        // Without a commit, what is staged is all there is to count.
        let since = if snapshot.is_unborn() {
            "--cached"
        } else {
            "HEAD"
        };
        snapshot.add_line_counts(&numstat(root, Some(since))?, Counted::SinceCommit);
        if wants.details {
            snapshot.add_line_counts(&numstat(root, Some("--cached"))?, Counted::Staged);
            snapshot.add_line_counts(&numstat(root, None)?, Counted::Unstaged);
        }
    }
    if wants.details {
        snapshot.details = Some(details::read(root, &snapshot));
        snapshot.operation = operation::read(root, git_directory);
        for (path, _) in snapshot.files.iter().filter(|(_, entry)| entry.conflicted) {
            let text = fs::metadata(root.join(path))
                .ok()
                .filter(|metadata| metadata.len() <= MAX_CONFLICT_FILE)
                .and_then(|_| fs::read_to_string(root.join(path)).ok());
            if let Some(text) = text {
                snapshot
                    .conflicts
                    .insert(path.clone(), conflict::parse(&text));
            }
        }
    }
    if let Some(history_wants) = wants.history.as_ref().filter(|_| !snapshot.is_unborn()) {
        snapshot.history = Some(history.read(root, history_wants, &snapshot)?);
    }
    for key in &wants.diffs {
        let settings = &wants.settings;
        if snapshot.untracked_directories.contains(&key.path) {
            let listing = untracked::list(root, &key.path, settings.untracked_max_files)?;
            snapshot.listings.insert(key.path.clone(), listing);
            continue;
        }
        if !key.staged && snapshot.is_untracked(&key.path) {
            // Gone since it was listed, it is left out until the status is read again.
            if let Ok(diff) = untracked::diff(root, &key.path, settings.untracked_max_bytes) {
                snapshot.diffs.insert(key.clone(), diff);
            }
            continue;
        }
        let Some(entry) = snapshot.files.get(&key.path) else {
            continue;
        };
        let mut command = git(root);
        command.args(["diff", "--no-color", "--no-ext-diff", "--no-textconv"]);
        if key.staged {
            command.arg("--cached");
        }
        command.arg("--").arg(&key.path);
        if key.staged {
            command.args(&entry.original);
        }
        let printed = output(&mut command)?;
        if let Some(diff) = parse_diff(&String::from_utf8_lossy(&printed)) {
            snapshot.diffs.insert(key.clone(), diff);
        }
    }
    Ok(snapshot)
}

/// `git diff --numstat` against `against`, else of what is not staged.
fn numstat(root: &Path, against: Option<&str>) -> io::Result<Vec<u8>> {
    let mut command = git(root);
    command.args(["diff", "--numstat", "-z", "--no-ext-diff"]);
    command.args(against);
    output(command.arg("--"))
}
