//! Buffers following their files: `revert-buffer`, and reading a file again when another
//! program changes it, like Emacs `global-auto-revert-mode`.

use std::collections::HashSet;
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use remacs_core::{Buffer, CommandResult, Editor, Notifier, quote};

use crate::project::abbreviate_home;

/// Reads the window buffer's file again, after asking if that drops unsaved changes.
pub fn revert_buffer(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    let path = buffer.path().ok_or("The buffer visits no file")?;
    if !buffer.is_modified() {
        return revert(editor);
    }
    let question = format!("Discard edits and read {} again?", quote(&abbreviate_home(path)));
    editor.ask_y_or_n(question, |editor, yes| if yes { revert(editor) } else { Ok(()) })
}

fn revert(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    buffer.revert()?;
    let message = format!("Reverted {}", buffer.name());
    editor.set_message(message);
    Ok(())
}

/// Whether unmodified buffers follow their files; `auto-revert` in `files.toml`.
pub struct AutoRevert {
    pub enabled: bool,
    watch: Option<Watch>,
}

impl Default for AutoRevert {
    fn default() -> Self {
        Self { enabled: true, watch: None }
    }
}

/// Watches the directories of the files buffers visit, waking the editor only for those
/// files.
struct Watch {
    watcher: RecommendedWatcher,
    shared: Arc<Mutex<Shared>>,
    /// The files buffers visit, in buffer order.
    files: Vec<PathBuf>,
    directories: HashSet<PathBuf>,
}

/// Between the watcher's thread and the editor.
#[derive(Default)]
struct Shared {
    /// The files buffers visit, which pick the events.
    files: HashSet<PathBuf>,
    /// Those changed since the editor last looked; the first one wakes it.
    changed: HashSet<PathBuf>,
}

impl Watch {
    fn start(notifier: Notifier) -> notify::Result<Self> {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let events = Arc::clone(&shared);
        let watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let Ok(event) = event else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let mut shared = events.lock().unwrap_or_else(PoisonError::into_inner);
            let quiet = shared.changed.is_empty();
            for path in event.paths {
                if shared.files.contains(&path) {
                    shared.changed.insert(path);
                }
            }
            let wake = quiet && !shared.changed.is_empty();
            drop(shared);
            if wake {
                notifier();
            }
        })?;
        Ok(Self { watcher, shared, files: Vec::new(), directories: HashSet::new() })
    }

    fn take_changed(&self) -> HashSet<PathBuf> {
        mem::take(&mut self.shared.lock().unwrap_or_else(PoisonError::into_inner).changed)
    }

    /// Watches the files `buffers` visit now, if they changed since the last call.
    fn follow(&mut self, buffers: &[Buffer]) {
        let paths = || buffers.iter().filter_map(Buffer::path).map(Path::as_os_str);
        // Runs on every refresh: comparing bytes in buffer order costs far less than hashing.
        if paths().eq(self.files.iter().map(|file| file.as_os_str())) {
            return;
        }
        self.files = paths().map(PathBuf::from).collect();
        let directories: HashSet<PathBuf> =
            self.files.iter().filter_map(|file| file.parent().map(Path::to_path_buf)).collect();
        // Restarting the stream is what costs; switching buffers only reorders them.
        if directories != self.directories {
            let mut watched = self.watcher.paths_mut();
            for gone in self.directories.difference(&directories) {
                let _ = watched.remove(gone);
            }
            // A directory not there yet stays unwatched: its files are new, nobody else's.
            for added in directories.difference(&self.directories) {
                let _ = watched.add(added, RecursiveMode::NonRecursive);
            }
            let _ = watched.commit();
            self.directories = directories;
        }
        self.shared.lock().unwrap_or_else(PoisonError::into_inner).files =
            self.files.iter().cloned().collect();
    }
}

/// Post-command hook: watches the files buffers visit, and brings each buffer whose file
/// changed up to date with it.
pub fn follow_files(editor: &mut Editor) {
    let state = editor.plugin_state::<AutoRevert>();
    if !state.enabled {
        state.watch = None;
        return;
    }
    let mut watch = match state.watch.take() {
        Some(watch) => watch,
        None => match Watch::start(editor.notifier()) {
            Ok(watch) => watch,
            Err(error) => {
                editor.plugin_state::<AutoRevert>().enabled = false;
                editor.set_error(format!("Auto-revert: {error}"));
                return;
            }
        },
    };
    watch.follow(editor.buffers());
    let changed = watch.take_changed();
    editor.plugin_state::<AutoRevert>().watch = Some(watch);
    if !changed.is_empty() {
        editor.sync_buffers_with_disk(|path| changed.contains(path));
    }
}
