use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ignore::WalkState;
use notify::event::ModifyKind;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use remacs_core::Notifier;

use crate::ignored::{FilesSettings, Ignored};

/// Keeps a prompt opened over a huge tree responsive and the index small; a tree this
/// big is indexed once and not kept current.
const MAX_FILES: usize = 100_000;
/// File system events arriving this close together are applied as one update.
const SETTLE_TIME: Duration = Duration::from_millis(50);
const IGNORE_FILES: &[&str] = &[".gitignore", ".ignore"];

/// One index per project root, kept until the settings of what is left out change.
#[derive(Default)]
pub struct FileIndexes {
    indexes: HashMap<PathBuf, Arc<FileIndex>>,
    settings: FilesSettings,
}

impl FileIndexes {
    /// `cache` is the directory the files are kept in between sessions, if any.
    pub fn get_or_start(
        &mut self,
        root: &Path,
        cache: Option<PathBuf>,
        notifier: Notifier,
    ) -> Arc<FileIndex> {
        let settings = &self.settings;
        Arc::clone(self.indexes.entry(root.to_path_buf()).or_insert_with(|| {
            let ignored = Arc::new(Ignored::new(root, settings));
            let cache = cache.map(|dir| dir.join(format!("files-{:x}", settings.key())));
            Arc::new(FileIndex::start(root.to_path_buf(), cache, notifier, ignored))
        }))
    }

    /// Drops the indexes when what they leave out changed, so that the next asks scan
    /// again.
    pub fn set_settings(&mut self, settings: FilesSettings) {
        if self.settings.key() != settings.key() {
            self.indexes.clear();
            self.settings = settings;
        }
    }
}

/// Files under a root, relative to it, without those the settings leave out; scanned on
/// a background thread and then kept current from file system events. The last scan's
/// files are cached, so that a huge tree is listed at once while it is scanned again.
pub struct FileIndex {
    listing: Arc<Mutex<Listing>>,
    messages: Sender<Message>,
}

#[derive(Default)]
struct Listing {
    files: Arc<Vec<String>>,
    /// The first scan finished; until then the files are the cached ones, or none.
    scanned: bool,
}

enum Message {
    Changed(notify::Result<Event>),
    Stop,
}

impl FileIndex {
    fn start(
        root: PathBuf,
        cache: Option<PathBuf>,
        notifier: Notifier,
        ignored: Arc<Ignored>,
    ) -> Self {
        let listing = Arc::default();
        let (messages, received) = mpsc::channel();
        let worker = Worker { root, cache, listing: Arc::clone(&listing), notifier, ignored };
        let events = messages.clone();
        thread::Builder::new()
            .name("file-index".into())
            .spawn(move || worker.run(events, received))
            .expect("spawning the file index thread");
        Self { listing, messages }
    }

    /// Shallow files first, then by path; the cached ones, or none, until the first scan
    /// finishes.
    pub fn files(&self) -> Arc<Vec<String>> {
        self.files_and_scanned().0
    }

    /// The files, and whether the first scan finished, so that they are all there.
    pub fn files_and_scanned(&self) -> (Arc<Vec<String>>, bool) {
        let listing = self.listing.lock().expect("index lock is never poisoned");
        (Arc::clone(&listing.files), listing.scanned)
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
    listing: Arc<Mutex<Listing>>,
    notifier: Notifier,
    ignored: Arc<Ignored>,
}

impl Worker {
    fn run(self, events: Sender<Message>, messages: Receiver<Message>) {
        if let Some(cached) = self.cache.as_deref().and_then(read_cache) {
            self.publish(cached, false);
        }
        // Watching before the scan keeps changes made during it.
        let watcher = notify::recommended_watcher(move |event| {
            let _ = events.send(Message::Changed(event));
        })
        .and_then(|mut watcher| {
            watcher.watch(&self.root, RecursiveMode::Recursive)?;
            Ok(watcher)
        });
        let mut tree = Tree::scan(&self.root, &self.ignored);
        self.publish_scan(&tree);
        if tree.truncated || watcher.is_err() {
            return;
        }
        while let Ok(Message::Changed(event)) = messages.recv() {
            let mut changes = Changes::default();
            changes.add(&self.root, event, &self.ignored);
            loop {
                match messages.recv_timeout(SETTLE_TIME) {
                    Ok(Message::Changed(event)) => changes.add(&self.root, event, &self.ignored),
                    Err(RecvTimeoutError::Timeout) => break,
                    Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            let rescan_all = changes.rescan_all;
            if !tree.apply(&self.root, &self.ignored, changes) {
                continue;
            }
            // Smaller changes are not worth rewriting a cache that may hold 100 000 paths;
            // the next session's scan finds them.
            if rescan_all {
                self.publish_scan(&tree);
            } else {
                self.publish(tree.files(), true);
            }
        }
    }

    fn publish_scan(&self, tree: &Tree) {
        let files = self.publish(tree.files(), true);
        if let Some(cache) = &self.cache {
            // Only a cache: when it cannot be written, the next session scans first.
            let _ = write_cache(cache, &files);
        }
    }

    /// `scanned` once the files come from a scan, not the cache.
    fn publish(&self, files: Vec<String>, scanned: bool) -> Arc<Vec<String>> {
        let files = Arc::new(files);
        *self.listing.lock().expect("index lock is never poisoned") =
            Listing { files: Arc::clone(&files), scanned };
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
    fn add(&mut self, root: &Path, event: notify::Result<Event>, ignored: &Ignored) {
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
            if ignored.uses_gitignore() && name.is_some_and(|name| IGNORE_FILES.contains(&name)) {
                self.rescan_all = true;
            } else if !ignored.is_ignored(path, path.is_dir())
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
    fn scan(root: &Path, ignored: &Arc<Ignored>) -> Self {
        Self::walk(root, ignored, "", None, MAX_FILES)
    }

    /// Directories entered under `directory` and their files, leaving out ignored ones;
    /// empty when `directory` is gone. Past `max_files` it keeps the shallowest whole
    /// levels of files, as a breadth-first walk would, however the parallel walk ran.
    fn walk(
        root: &Path,
        ignored: &Arc<Ignored>,
        directory: &str,
        max_depth: Option<usize>,
        max_files: usize,
    ) -> Self {
        let walk = Mutex::new(CappedWalk {
            tree: Self::default(),
            max_files,
            max_file_depth: usize::MAX,
            files_at_depth: Vec::new(),
        });
        ignored.walker(&root.join(directory)).max_depth(max_depth).build_parallel().run(|| {
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
                let depth = relative.components().count();
                let mut walk = walk.lock().expect("walker threads do not panic");
                if kind.is_dir() {
                    if depth >= walk.max_file_depth {
                        return WalkState::Skip;
                    }
                    walk.tree.directories.entry(path_key(relative)).or_default();
                } else if kind.is_file() && entry.depth() > 0 {
                    let parent = relative.parent().map(path_key).unwrap_or_default();
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if !walk.add_file(parent, name, depth) {
                        return WalkState::Quit;
                    }
                }
                WalkState::Continue
            })
        });
        walk.into_inner().expect("walker threads do not panic").tree
    }

    /// Brings the changed directories up to date; false when nothing changed.
    fn apply(&mut self, root: &Path, ignored: &Arc<Ignored>, changes: Changes) -> bool {
        if changes.rescan_all {
            *self = Self::scan(root, ignored);
            return true;
        }
        // Parents sort before their subdirectories, so a new directory is walked once.
        changes
            .directories
            .iter()
            .fold(false, |changed, directory| self.rescan(root, ignored, directory) | changed)
    }

    /// Lists `directory` again, walking subdirectories that appeared in it. Directories not
    /// in the index are ignored ones, or new ones that their parent's rescan picks up.
    fn rescan(&mut self, root: &Path, ignored: &Arc<Ignored>, directory: &str) -> bool {
        let Some(old_files) = self.directories.get(directory) else {
            return false;
        };
        let mut listing = Self::walk(root, ignored, directory, Some(1), usize::MAX);
        let Some(mut files) = listing.directories.remove(directory) else {
            self.remove_subtree(directory);
            return true;
        };
        files.sort();
        let mut changed = *old_files != files;
        let known: BTreeSet<String> = self.subdirectories(directory).cloned().collect();
        for gone in known.iter().filter(|key| !listing.directories.contains_key(*key)) {
            self.remove_subtree(gone);
            changed = true;
        }
        for added in listing.directories.keys().filter(|key| !known.contains(*key)) {
            let remaining = MAX_FILES.saturating_sub(self.file_count);
            let subtree = Self::walk(root, ignored, added, None, remaining);
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
        self.directories.keys().filter(move |key| parent_key(key) == Some(directory))
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
                    if directory.is_empty() { name.clone() } else { format!("{directory}/{name}") }
                })
            })
            .collect();
        let depth = |path: &String| path.matches('/').count();
        files.sort_by(|a, b| depth(a).cmp(&depth(b)).then_with(|| a.cmp(b)));
        files
    }
}

/// A walk's tree while it is capped: always every file seen down to `max_file_depth`,
/// which drops a level at a time as the cap is reached.
struct CappedWalk {
    tree: Tree,
    max_files: usize,
    max_file_depth: usize,
    /// Kept files per depth, a file in the root being at depth 1; no trailing zeros.
    files_at_depth: Vec<usize>,
}

impl CappedWalk {
    /// False when the files of the shallowest level alone are over the cap, so the walk
    /// stops with as many of them as fit.
    fn add_file(&mut self, parent: String, name: String, depth: usize) -> bool {
        if depth > self.max_file_depth {
            return true;
        }
        if self.tree.file_count == self.max_files {
            self.tree.truncated = true;
            let deepest = self.files_at_depth.len().saturating_sub(1);
            if depth > deepest {
                self.max_file_depth = depth - 1;
                return true;
            }
            if deepest <= 1 {
                return false;
            }
            self.drop_level(deepest);
            if depth == deepest {
                return true;
            }
        }
        if self.files_at_depth.len() <= depth {
            self.files_at_depth.resize(depth + 1, 0);
        }
        self.files_at_depth[depth] += 1;
        self.tree.directories.entry(parent).or_default().push(name);
        self.tree.file_count += 1;
        true
    }

    fn drop_level(&mut self, depth: usize) {
        self.max_file_depth = depth - 1;
        self.tree.directories.retain(|key, _| Path::new(key).components().count() < depth - 1);
        self.tree.file_count -= self.files_at_depth[depth];
        self.files_at_depth.truncate(depth);
        while self.files_at_depth.last() == Some(&0) {
            self.files_at_depth.pop();
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_over_the_cap_keeps_its_shallowest_whole_levels() {
        let root = std::env::temp_dir().join(format!("remacs-capped-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let deep = (0..20).map(|n| format!("go/pkg/mod/dep/{n}.go"));
        let shallow = ["ui.toml", ".config/remacs/config/ui.toml", "notes/todo.md"];
        for file in shallow.iter().map(|file| file.to_string()).chain(deep) {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        let ignored = Arc::new(Ignored::new(&root, &FilesSettings::default()));

        let tree = Tree::walk(&root, &ignored, "", None, 10);

        assert!(tree.truncated);
        assert_eq!(tree.files(), ["ui.toml", "notes/todo.md", ".config/remacs/config/ui.toml"]);
        assert_eq!(tree.file_count, 3);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_root_alone_over_the_cap_keeps_as_many_of_its_files_as_fit() {
        let root = std::env::temp_dir().join(format!("remacs-flat-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("deep")).unwrap();
        fs::write(root.join("deep/file.txt"), "").unwrap();
        for n in 0..15 {
            fs::write(root.join(format!("{n}.txt")), "").unwrap();
        }
        let ignored = Arc::new(Ignored::new(&root, &FilesSettings::default()));

        let tree = Tree::walk(&root, &ignored, "", None, 10);

        assert!(tree.truncated);
        assert_eq!(tree.file_count, 10);
        assert!(tree.files().iter().all(|file| !file.contains('/')));
        let _ = fs::remove_dir_all(&root);
    }
}
