//! `cargo bench -p remacs-git --bench status`

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use divan::{Bencher, black_box};
use remacs_core::{Editor, OutlineRow, TextSide, parse_sequence};
use remacs_dired::DiredPlugin;
use remacs_editing::EditingPlugin;
use remacs_files::FilesPlugin;
use remacs_git::GitPlugin;
use remacs_minibuffer::MinibufferPlugin;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const TIMEOUT: Duration = Duration::from_secs(120);

fn main() {
    divan::main();
}

/// A repository with `files` committed files, each changed since, and `untracked` files of
/// 20 lines among them, so Git lists each; removed on drop.
struct Repository(PathBuf);

impl Repository {
    fn new(files: usize, untracked: usize) -> Self {
        let root = std::env::temp_dir().join(format!("remacs-bench-status-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let repository = Self(root.canonicalize().unwrap());
        repository.git(&["init", "-q", "-b", "main"]);
        repository.git(&["config", "user.email", "bench@example.com"]);
        repository.git(&["config", "user.name", "Bench"]);
        let path = |file: usize| {
            let directory = repository.0.join(format!("src/module_{}", file % 100));
            fs::create_dir_all(&directory).unwrap();
            directory.join(format!("file_{file}.rs"))
        };
        for file in 0..files {
            fs::write(path(file), "line\n").unwrap();
        }
        repository.git(&["add", "."]);
        repository.git(&["commit", "-q", "-m", "First"]);
        for file in 0..files {
            fs::write(path(file), "line\nchanged\n").unwrap();
        }
        let lines: String = (0..20).map(|line| format!("let line_{line} = {line};\n")).collect();
        for file in files..files + untracked {
            fs::write(path(file), &lines).unwrap();
        }
        repository
    }

    fn git(&self, args: &[&str]) {
        let status = Command::new("git").current_dir(&self.0).args(args).status().unwrap();
        assert!(status.success(), "git {args:?}");
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

/// What drawing the changes of `files` files again takes: here after their diffs are
/// folded or unfolded, as after every new status read.
#[divan::bench(args = [3000], sample_count = 10)]
fn redraw_changes(bencher: Bencher, files: usize) {
    let repository = Repository::new(files, 0);
    let mut editor = status(&repository, files + 1);
    let toggle = parse_sequence("S-TAB").unwrap();
    bencher.bench_local(|| {
        for key in &toggle {
            editor.handle_key(*key);
        }
        black_box(editor.outline(0..40));
    });
}

/// What the status takes to show a file just made, among `untracked` other new files,
/// as after any save while the changes are shown.
#[divan::bench(args = [300], sample_count = 10)]
fn refresh_with_untracked(bencher: Bencher, untracked: usize) {
    const CHANGED: usize = 100;
    let repository = Repository::new(CHANGED, untracked);
    let mut editor = status(&repository, CHANGED + untracked + 2);
    let mut made = 0;
    bencher.bench_local(|| {
        made += 1;
        let name = format!("made_{made}.rs");
        fs::write(repository.0.join(&name), "new\n").unwrap();
        wait_until(&mut editor, &name, |editor| lists(editor, &name));
    });
}

/// The status of `repository`, once it lists `rows` headings and files.
fn status(repository: &Repository, rows: usize) -> Editor {
    let mut editor =
        Editor::new(&[&EditingPlugin, &MinibufferPlugin, &FilesPlugin, &DiredPlugin, &GitPlugin]);
    editor.viewport.set_height(40);
    editor.visit_file(&repository.0.join("src/module_0/file_0.rs")).unwrap();
    editor.run_command("git-status");
    wait_until(&mut editor, "the changes", |editor| {
        side(editor).is_some_and(|side| side.rows.len() == rows)
    });
    editor
}

fn side(editor: &Editor) -> Option<TextSide> {
    editor.outline(0..1).and_then(|outline| outline.side)
}

/// Whether the changes list `name`, its diff read.
fn lists(editor: &Editor, name: &str) -> bool {
    let shown =
        |row: &OutlineRow| matches!(row, OutlineRow::File { name: listed, .. } if listed == name);
    side(editor).is_some_and(|side| side.rows.iter().any(shown))
}
