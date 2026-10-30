//! A test binary of its own: dhat replaces the global allocator.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{Editor, OutlineRow};
use remacs_dired::{DiredPlugin, list_directory};
use remacs_editing::EditingPlugin;
use remacs_files::FilesPlugin;
use remacs_git::GitPlugin;
use remacs_minibuffer::MinibufferPlugin;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const FILES: usize = 50;
const LINES: usize = 200;
const SNAPSHOTS: usize = 5;
const TIMEOUT: Duration = Duration::from_secs(30);

/// A throwaway repository whose `src/` files are all rewritten since the commit.
struct Repository(PathBuf);

impl Repository {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("remacs-git-heap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        let repository = Self(root.canonicalize().unwrap());
        repository.git(&["init", "-q", "-b", "main"]);
        repository.git(&["config", "user.email", "test@example.com"]);
        repository.git(&["config", "user.name", "Test"]);
        repository.write_sources("old");
        repository.git(&["add", "."]);
        repository.git(&["commit", "-q", "-m", "First"]);
        repository.write_sources("new");
        repository
    }

    fn git(&self, args: &[&str]) {
        let status = Command::new("git").current_dir(&self.0).args(args).status().unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn write_sources(&self, version: &str) {
        for file in 0..FILES {
            let text: String =
                (0..LINES).map(|line| format!("{version} line {line} of file {file}\n")).collect();
            fs::write(self.0.join(format!("src/f{file}.rs")), text).unwrap();
        }
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wait_until(editor: &mut Editor, what: &str, done: impl Fn(&Editor) -> bool) {
    let start = Instant::now();
    while !done(editor) {
        assert!(start.elapsed() < TIMEOUT, "{what} never happened");
        thread::sleep(Duration::from_millis(10));
        editor.refresh();
    }
}

/// The files listed at the left of their diffs.
fn last_rows(editor: &Editor) -> Vec<OutlineRow> {
    editor
        .outline(0..1)
        .and_then(|outline| outline.side)
        .map_or_else(Vec::new, |side| side.rows.to_vec())
}

fn lists(rows: &[OutlineRow], name: &str) -> bool {
    rows.iter().any(|row| matches!(row, OutlineRow::File { name: listed, .. } if listed == name))
}

/// Bytes allocated per snapshot, the reading included, until `shown` says it is.
fn bytes_per_snapshot(
    editor: &mut Editor,
    root: &Path,
    kind: &str,
    shown: impl Fn(&Editor, &str) -> bool,
) -> u64 {
    let before = dhat::HeapStats::get().total_bytes;
    for snapshot in 0..SNAPSHOTS {
        let name = format!("trigger-{kind}-{snapshot}");
        fs::write(root.join(&name), "").unwrap();
        wait_until(editor, &name, |editor| shown(editor, &name));
    }
    (dhat::HeapStats::get().total_bytes - before) / SNAPSHOTS as u64
}

#[test]
fn a_status_snapshot_with_every_diff_unfolded_is_drawn_once_and_only_when_shown() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let repository = Repository::new();
    let mut editor =
        Editor::new(&[&EditingPlugin, &MinibufferPlugin, &FilesPlugin, &DiredPlugin, &GitPlugin]);
    editor.viewport.set_height(20);
    editor.visit_file(&repository.0.join("src/f0.rs")).unwrap();
    editor.run_command("git-status");
    wait_until(&mut editor, "the status", |editor| {
        lists(&last_rows(editor), &format!("f{}.rs", FILES - 1))
    });
    wait_until(&mut editor, "every diff", |editor| {
        editor.window_buffer().text().line_count() > FILES * LINES * 2
    });

    let shown = bytes_per_snapshot(&mut editor, &repository.0, "shown", |editor, name| {
        lists(&last_rows(editor), name)
    });
    list_directory(&mut editor, &repository.0).unwrap();
    let hidden = bytes_per_snapshot(&mut editor, &repository.0, "hidden", |editor, name| {
        editor.table(0..20).is_some_and(|table| {
            table.rows.iter().any(|row| row.cells[0] == name && !row.cells[1].is_empty())
        })
    });
    eprintln!(
        "bytes per snapshot: {shown} with the status shown, {hidden} with it hidden; peak {}",
        dhat::HeapStats::get().max_bytes
    );
    dhat::assert!(hidden * 4 < shown * 3);

    editor.run_command("git-status");
    let last = format!("trigger-hidden-{}", SNAPSHOTS - 1);
    wait_until(&mut editor, "the status drawn again", |editor| lists(&last_rows(editor), &last));
}
