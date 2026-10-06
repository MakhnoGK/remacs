use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ignore::{WalkBuilder, WalkState};
use notify::event::ModifyKind;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use remacs_core::Notifier;

/// Keeps a prompt opened over a huge tree responsive and the index small; a tree this
/// big is indexed once and not kept current.
const MAX_FILES: usize = 100_000;
/// File system events arriving this close together are applied as one update.
const SETTLE_TIME: Duration = Duration::from_millis(50);
const IGNORE_FILES: &[&str] = &[".gitignore", ".ignore"];

/// One index per project root, kept for the editor's lifetime.
#[derive(Default)]
pub struct FileIndexes(HashMap<PathBuf, Arc<FileIndex>>);

impl FileIndexes {
    /// `cache` is the file the files are kept in between sessions, if any.
    pub fn get_or_start(
        &mut self,
        root: &Path,
        cache: Option<PathBuf>,
        notifier: Notifier,
    ) -> Arc<FileIndex> {
        Arc::clone(
            self.0
                .entry(root.to_path_buf())
                .or_insert_with(|| Arc::new(FileIndex::start(root.to_path_buf(), cache, notifier))),
        )
    }
}

/// Files under a root, relative to it, without hidden and ignored ones; scanned on a
/// background thread and then kept current from file system events. The last scan's
/// files are cached, so that a huge tree is listed at once while it is scanned again.
pub struct FileIndex {
    files: Arc<Mutex<Arc<Vec<String>>>>,
    messages: Sender<Message>,
}

enum Message {
    Changed(notify::Result<Event>),
    Stop,
}

impl FileIndex {
    fn start(root: PathBuf, cache: Option<PathBuf>, notifier: Notifier) -> Self {
        let files = Arc::default();
        let (messages, received) = mpsc::channel();
        let worker = Worker {
            root,
            cache,
            files: Arc::clone(&files),
            notifier,
        };
        let events = messages.clone();
        thread::Builder::new()
            .name("file-index".into())
            .spawn(move || worker.run(events, received))
            .expect("spawning the file index thread");
        Self { files, messages }
    }

    /// Shallow files first, then by path; the cached ones, or none, until the first scan
    /// finishes.
    pub fn files(&self) -> Arc<Vec<String>> {
        Arc::clone(&self.files.lock().expect("index lock is never poisoned"))
    }
}

impl Drop for FileIndex {
    fn drop(&mut self) {
        let _ = self.messages.send(Message::Stop);
    }
}

struct Worker {
    root: PathBuf,
    cache: Option<PathBuf>,
    files: Arc<Mutex<Arc<Vec<String>>>>,
    notifier: Notifier,
}

impl Worker {
    fn run(self, events: Sender<Message>, messages: Receiver<Message>) {
        if let Some(cached) = self.cache.as_deref().and_then(read_cache) {
            self.publish(cached);
        }
        // Watching before the scan keeps changes made during it.
        let watcher = notify::recommended_watcher(move |event| {
            let _ = events.send(Message::Changed(event));
        })
        .and_then(|mut watcher| {
            watcher.watch(&self.root, RecursiveMode::Recursive)?;
            Ok(watcher)
        });
        let mut tree = Tree::scan(&self.root);
        self.publish_scan(&tree);
        if tree.truncated || watcher.is_err() {
            return;
        }
        while let Ok(Message::Changed(event)) = messages.recv() {
            let mut changes = Changes::default();
            changes.add(&self.root, event);
            loop {
                match messages.recv_timeout(SETTLE_TIME) {
                    Ok(Message::Changed(event)) => changes.add(&self.root, event),
                    Err(RecvTimeoutError::Timeout) => break,
                    Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            let rescan_all = changes.rescan_all;
            if !tree.apply(&self.root, changes) {
                continue;
            }
            // Smaller changes are not worth rewriting a cache that may hold 100 000 paths;
            // the next session's scan finds them.
            if rescan_all {
                self.publish_scan(&tree);
            } else {
                self.publish(tree.files());
            }
        }
    }

    fn publish_scan(&self, tree: &Tree) {
        let files = self.publish(tree.files());
        if let Some(cache) = &self.cache {
            // Only a cache: when it cannot be written, the next session scans first.
            let _ = write_cache(cache, &files);
        }
    }

    fn publish(&self, files: Vec<String>) -> Arc<Vec<String>> {
        let files = Arc::new(files);
        *self.files.lock().expect("index lock is never poisoned") = Arc::clone(&files);
        (self.notifier)();
        files
    }
}

/// One path per line; none when the file is missing or empty.
fn read_cache(path: &Path) -> Option<Vec<String>> {
    let text = fs::read_to_string(path).ok()?;
    let files: Vec<String> = text.lines().map(str::to_owned).collect();
    (!files.is_empty()).then_some(files)
}

/// Through a temporary file, so that a crash never leaves half a list.
fn write_cache(path: &Path, files: &[String]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut text = files.join("\n");
    text.push('\n');
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, text)?;
    fs::rename(temporary, path)
}

/// Directories whose listings may have changed.
#[derive(Default)]
struct Changes {
    rescan_all: bool,
    directories: BTreeSet<String>,
}

impl Changes {
    fn add(&mut self, root: &Path, event: notify::Result<Event>) {
        let Ok(event) = event else {
            self.rescan_all = true;
            return;
        };
        if event.need_rescan() {
            self.rescan_all = true;
        }
        if matches!(
            event.kind,
            EventKind::Access(_) | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_))
        ) {
            return;
        }
        for path in &event.paths {
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let name = relative.file_name().and_then(|name| name.to_str());
            if name.is_some_and(|name| IGNORE_FILES.contains(&name)) {
                self.rescan_all = true;
            } else if !is_hidden(relative)
                && let Some(parent) = relative.parent()
            {
                self.directories.insert(path_key(parent));
            }
        }
    }
}

/// The indexed directories, relative to the root (`""` for it), with the names of their files.
#[derive(Default)]
struct Tree {
    directories: BTreeMap<String, Vec<String>>,
    file_count: usize,
    truncated: bool,
}

impl Tree {
    fn scan(root: &Path) -> Self {
        Self::walk(root, "", None, MAX_FILES)
    }

    /// Directories entered under `directory` and their files, honouring ignore files;
    /// empty when `directory` is gone.
    fn walk(root: &Path, directory: &str, max_depth: Option<usize>, max_files: usize) -> Self {
        let tree = Mutex::new(Self::default());
        WalkBuilder::new(root.join(directory))
            .max_depth(max_depth)
            .build_parallel()
            .run(|| {
                Box::new(|entry| {
                    let Ok(entry) = entry else {
                        return WalkState::Continue;
                    };
                    let Some(kind) = entry.file_type() else {
                        return WalkState::Continue;
                    };
                    let Ok(relative) = entry.path().strip_prefix(root) else {
                        return WalkState::Continue;
                    };
                    let mut tree = tree.lock().expect("walker threads do not panic");
                    if kind.is_dir() {
                        tree.directories.entry(path_key(relative)).or_default();
                    } else if kind.is_file() && entry.depth() > 0 {
                        if tree.file_count == max_files {
                            tree.truncated = true;
                            return WalkState::Quit;
                        }
                        let parent = relative.parent().map(path_key).unwrap_or_default();
                        let name = entry.file_name().to_string_lossy().into_owned();
                        tree.directories.entry(parent).or_default().push(name);
                        tree.file_count += 1;
                    }
                    WalkState::Continue
                })
            });
        tree.into_inner().expect("walker threads do not panic")
    }

    /// Brings the changed directories up to date; false when nothing changed.
    fn apply(&mut self, root: &Path, changes: Changes) -> bool {
        if changes.rescan_all {
            *self = Self::scan(root);
            return true;
        }
        // Parents sort before their subdirectories, so a new directory is walked once.
        changes
            .directories
            .iter()
            .fold(false, |changed, directory| {
                self.rescan(root, directory) | changed
            })
    }

    /// Lists `directory` again, walking subdirectories that appeared in it. Directories not
    /// in the index are ignored ones, or new ones that their parent's rescan picks up.
    fn rescan(&mut self, root: &Path, directory: &str) -> bool {
        let Some(old_files) = self.directories.get(directory) else {
            return false;
        };
        let mut listing = Self::walk(root, directory, Some(1), usize::MAX);
        let Some(mut files) = listing.directories.remove(directory) else {
            self.remove_subtree(directory);
            return true;
        };
        files.sort();
        let mut changed = *old_files != files;
        let known: BTreeSet<String> = self.subdirectories(directory).cloned().collect();
        for gone in known
            .iter()
            .filter(|key| !listing.directories.contains_key(*key))
        {
            self.remove_subtree(gone);
            changed = true;
        }
        for added in listing
            .directories
            .keys()
            .filter(|key| !known.contains(*key))
        {
            let remaining = MAX_FILES.saturating_sub(self.file_count);
            let subtree = Self::walk(root, added, None, remaining);
            self.file_count += subtree.file_count;
            self.directories.extend(subtree.directories);
            changed = true;
        }
        let old_count = self.directories.insert(directory.to_owned(), files);
        self.file_count -= old_count.map_or(0, |files| files.len());
        self.file_count += self.directories[directory].len();
        changed
    }

    fn subdirectories<'a>(&'a self, directory: &'a str) -> impl Iterator<Item = &'a String> {
        self.directories
            .keys()
            .filter(move |key| parent_key(key) == Some(directory))
    }

    fn remove_subtree(&mut self, directory: &str) {
        let prefix = format!("{directory}/");
        let mut removed_files = 0;
        self.directories.retain(|key, files| {
            let inside = directory.is_empty() || key == directory || key.starts_with(&prefix);
            if inside {
                removed_files += files.len();
            }
            !inside
        });
        self.file_count -= removed_files;
    }

    fn files(&self) -> Vec<String> {
        let mut files: Vec<String> = self
            .directories
            .iter()
            .flat_map(|(directory, names)| {
                names.iter().map(move |name| {
                    if directory.is_empty() {
                        name.clone()
                    } else {
                        format!("{directory}/{name}")
                    }
                })
            })
            .collect();
        let depth = |path: &String| path.matches('/').count();
        files.sort_by(|a, b| depth(a).cmp(&depth(b)).then_with(|| a.cmp(b)));
        files
    }
}

fn path_key(relative: &Path) -> String {
    relative.to_string_lossy().into_owned()
}

fn parent_key(key: &str) -> Option<&str> {
    match key.rfind('/') {
        Some(slash) => Some(&key[..slash]),
        None => (!key.is_empty()).then_some(""),
    }
}

fn is_hidden(relative: &Path) -> bool {
    relative
        .components()
        .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
}
