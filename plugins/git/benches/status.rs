//! `cargo bench -p remacs-git --bench status`

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use divan::{Bencher, black_box};
use remacs_core::{Editor, parse_sequence};
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

/// A repository with `files` committed files, each changed since, removed on drop.
struct Repository(PathBuf);

impl Repository {
    fn new(files: usize) -> Self {
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
        repository
    }

    fn git(&self, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(&self.0)
            .args(args)
            .status()
            .unwrap();
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
    let repository = Repository::new(files);
    let mut editor = Editor::new(&[
        &EditingPlugin,
        &MinibufferPlugin,
        &FilesPlugin,
        &DiredPlugin,
        &GitPlugin,
    ]);
    editor.viewport.set_height(40);
    editor
        .visit_file(&repository.0.join("src/module_0/file_0.rs"))
        .unwrap();
    editor.run_command("git-status");
    wait_until(&mut editor, "the changes", |editor| {
        editor
            .outline(0..1)
            .and_then(|outline| outline.side)
            .is_some_and(|side| side.rows.len() == files + 1)
    });
    let toggle = parse_sequence("S-TAB").unwrap();
    bencher.bench_local(|| {
        for key in &toggle {
            editor.handle_key(*key);
        }
        black_box(editor.outline(0..40));
    });
}
