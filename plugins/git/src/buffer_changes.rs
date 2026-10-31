//! How the lines of the buffer in the window differ from the last commit, unsaved edits
//! included, like Emacs diff-hl: compared on a background thread with the file as
//! checking out the commit would write it.

use std::ffi::OsString;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use imara_diff::{Algorithm, Diff, InternedInput};
use remacs_core::{Buffer, ChangedLines, Editor, FileState, LineChange, Notifier};
use remacs_text::Snapshot;

use crate::command::{git, output};
use crate::repositories::Repositories;

/// Each edit copies and compares the whole buffer, so bigger ones are left out.
const MAX_COMPARED_BYTES: usize = 16 << 20;

/// What a buffer is compared with.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Base {
    root: PathBuf,
    /// Relative to the root, as named in the commit: a renamed file's name before.
    relative: PathBuf,
    /// `None` for a file no commit has, so all of its lines are added.
    commit: Option<String>,
}

/// Where the compare thread leaves a buffer's changes.
type ChangesSlot = Arc<Mutex<Compared>>;

#[derive(Default)]
struct Compared {
    /// The base last read, with its text; none when the commit has no such file.
    base: Option<(Base, Option<Arc<[u8]>>)>,
    changes: Vec<ChangedLines>,
}

/// Kept in a buffer while it is compared.
struct BufferChanges {
    base: Base,
    /// The text last sent to be compared.
    sent: Snapshot,
    slot: ChangesSlot,
}

struct Request {
    base: Base,
    text: Snapshot,
    slot: ChangesSlot,
}

/// One thread compares for every buffer; it starts with the first request.
#[derive(Default)]
pub struct CompareThread(Option<Sender<Request>>);

impl CompareThread {
    fn send(&mut self, request: Request, notifier: Notifier) {
        let requests = self.0.get_or_insert_with(|| {
            let (requests, received) = mpsc::channel();
            thread::Builder::new()
                .name("git-changes".into())
                .spawn(move || run(received, notifier))
                .expect("spawning the git changes thread");
            requests
        });
        // The thread only stops when this sender is dropped.
        let _ = requests.send(request);
    }
}

/// Sends the window buffer to be compared when its text or its base changed. Runs after
/// every command.
pub fn sync(editor: &mut Editor) {
    let base =
        base_of(editor).filter(|_| editor.window_buffer().text().len() <= MAX_COMPARED_BYTES);
    let buffer = editor.window_buffer_mut();
    let Some(base) = base else {
        buffer.remove_local::<BufferChanges>();
        return;
    };
    let text = buffer.text().snapshot();
    let request = match buffer.local_mut::<BufferChanges>() {
        Some(changes) if changes.base == base && changes.sent.is_same(&text) => return,
        Some(changes) => {
            changes.base = base.clone();
            changes.sent = text.clone();
            Request { base, text, slot: Arc::clone(&changes.slot) }
        }
        None => {
            let slot = ChangesSlot::default();
            buffer.set_local(BufferChanges {
                base: base.clone(),
                sent: text.clone(),
                slot: Arc::clone(&slot),
            });
            Request { base, text, slot }
        }
    };
    let notifier = editor.notifier();
    editor.plugin_state::<CompareThread>().send(request, notifier);
}

pub fn line_changes(buffer: &Buffer, lines: Range<usize>) -> Vec<ChangedLines> {
    let Some(changes) = buffer.local::<BufferChanges>() else {
        return Vec::new();
    };
    lock(&changes.slot)
        .changes
        .iter()
        .filter(|changed| overlaps(changed, &lines))
        .cloned()
        .collect()
}

/// The lines of `text` that differ from `base`: in each hunk, as many lines as were
/// removed count as modified, the rest as added, or as a removal after them when fewer
/// took the removed lines' place.
pub fn compare(base: &[u8], text: &[u8]) -> Vec<ChangedLines> {
    let input = InternedInput::new(base, text);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    diff.hunks()
        .flat_map(|hunk| {
            let (removed, added) = (hunk.before.len(), hunk.after.len());
            let start = hunk.after.start as usize;
            let end = hunk.after.end as usize;
            let modified_end = start + removed.min(added);
            [(start..modified_end, LineChange::Modified), (modified_end..end, LineChange::Added)]
                .into_iter()
                .filter(|(lines, _)| !lines.is_empty())
                .chain((removed > added).then_some((end..end, LineChange::Removed)))
                .map(|(lines, change)| ChangedLines { lines, change })
        })
        .collect()
}

/// Removed lines show between two lines, so also at the end of `lines`.
fn overlaps(changed: &ChangedLines, lines: &Range<usize>) -> bool {
    if changed.lines.is_empty() {
        (lines.start..=lines.end).contains(&changed.lines.start)
    } else {
        changed.lines.start < lines.end && lines.start < changed.lines.end
    }
}

/// The window buffer's file in the commit `HEAD` is at; nothing for a file Git ignores.
fn base_of(editor: &Editor) -> Option<Base> {
    let path = editor.window_buffer().path()?;
    let repositories = editor.find_plugin_state::<Repositories>()?;
    let (root, repository, relative) = repositories.find(path)?;
    let snapshot = repository.snapshot.as_ref()?;
    let commit = match snapshot.status(relative).map(|status| status.state) {
        Some(FileState::Untracked | FileState::Added) => None,
        _ => Some(snapshot.commit.clone()?),
    };
    let original = snapshot.files.get(relative).and_then(|entry| entry.original.clone());
    Some(Base {
        root: root.to_path_buf(),
        relative: original.unwrap_or_else(|| relative.to_path_buf()),
        commit,
    })
}

fn run(requests: Receiver<Request>, notifier: Notifier) {
    while let Ok(first) = requests.recv() {
        for request in newest_per_buffer(first, &requests) {
            // The buffer was killed or stopped being compared.
            if Arc::strong_count(&request.slot) == 1 {
                continue;
            }
            let changes = match base_text(&request) {
                Some(base) => compare(
                    &request.text.format().decode(&base),
                    &request.text.bytes_in(0..request.text.len()),
                ),
                None => Vec::new(),
            };
            // Each wake costs the UI a full refresh; most keys change no line's mark.
            let mut compared = lock(&request.slot);
            if compared.changes != changes {
                compared.changes = changes;
                drop(compared);
                notifier();
            }
        }
    }
}

/// Requests queued while the previous comparison ran; older ones for the same buffer are
/// stale.
fn newest_per_buffer(first: Request, requests: &Receiver<Request>) -> Vec<Request> {
    let mut newest: Vec<Request> = Vec::new();
    for request in std::iter::once(first).chain(requests.try_iter()) {
        match newest.iter_mut().find(|queued| Arc::ptr_eq(&queued.slot, &request.slot)) {
            Some(queued) => *queued = request,
            None => newest.push(request),
        }
    }
    newest
}

/// The request's base text, read once per base.
fn base_text(request: &Request) -> Option<Arc<[u8]>> {
    if let Some((base, text)) = &lock(&request.slot).base
        && *base == request.base
    {
        return text.clone();
    }
    let text = read_base(&request.base).map(Arc::from);
    lock(&request.slot).base = Some((request.base.clone(), text.clone()));
    text
}

/// With the filters checkout applies, so line endings and smudged contents compare equal.
fn read_base(base: &Base) -> Option<Vec<u8>> {
    let Some(commit) = &base.commit else {
        return Some(Vec::new());
    };
    let mut object = OsString::from(format!("{commit}:"));
    object.push(&base.relative);
    output(git(&base.root).args(["cat-file", "--filters"]).arg(object)).ok()
}

fn lock(slot: &ChangesSlot) -> MutexGuard<'_, Compared> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed(lines: Range<usize>, change: LineChange) -> ChangedLines {
        ChangedLines { lines, change }
    }

    #[test]
    fn hunks_are_modified_lines_then_added_ones_or_a_removal() {
        let base = b"a\nb\nc\nd\ne\nf\n";
        let text = b"a\nB\nX\nY\nc\nd\nf\n";
        assert_eq!(
            compare(base, text),
            [
                changed(1..2, LineChange::Modified),
                changed(2..4, LineChange::Added),
                changed(6..6, LineChange::Removed),
            ]
        );
        assert_eq!(
            compare(b"a\nb\nc\n", b"a\nB\n"),
            [changed(1..2, LineChange::Modified), changed(2..2, LineChange::Removed),]
        );
        assert_eq!(compare(b"", b"a\nb\n"), [changed(0..2, LineChange::Added)]);
        assert!(compare(b"a\n", b"a\n").is_empty());
    }

    #[test]
    fn removals_show_at_either_edge_of_the_lines_asked_for() {
        let removed = changed(5..5, LineChange::Removed);
        assert!(overlaps(&removed, &(0..5)) && overlaps(&removed, &(5..9)));
        assert!(!overlaps(&removed, &(6..9)));
        let added = changed(5..7, LineChange::Added);
        assert!(overlaps(&added, &(6..9)) && !overlaps(&added, &(0..5)));
    }
}
