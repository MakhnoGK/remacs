//! `cargo bench -p remacs-git --bench commit_view`

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

/// A repository whose last commit adds `files` files of one line, removed on drop.
struct Repository(PathBuf);

impl Repository {
    fn new(files: usize) -> Self {
        let root = std::env::temp_dir().join(format!("remacs-bench-commit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let repository = Self(root.canonicalize().unwrap());
        repository.git(&["init", "-q", "-b", "main"]);
        repository.git(&["config", "user.email", "bench@example.com"]);
        repository.git(&["config", "user.name", "Bench"]);
        fs::write(repository.0.join("lib.rs"), "one\n").unwrap();
        repository.git(&["add", "."]);
        repository.git(&["commit", "-q", "-m", "First"]);
        for file in 0..files {
            let directory = repository.0.join(format!("vendor/crate_{}", file % 100));
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join(format!("file_{file}.rs")), "line\n").unwrap();
        }
        repository.git(&["add", "."]);
        repository.git(&["commit", "-q", "-m", "Vendor"]);
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

/// What every frame asks of a commit of `files` files: the column beside its diff.
#[divan::bench(args = [3000], sample_count = 50)]
fn side_column(bencher: Bencher, files: usize) {
    let repository = Repository::new(files);
    let mut editor =
        Editor::new(&[&EditingPlugin, &MinibufferPlugin, &FilesPlugin, &DiredPlugin, &GitPlugin]);
    editor.viewport.set_height(40);
    editor.visit_file(&repository.0.join("lib.rs")).unwrap();
    editor.run_command("git-status");
    wait_until(&mut editor, "the history", |editor| {
        editor
            .outline(0..100)
            .and_then(|outline| outline.history)
            .is_some_and(|history| history.commits.len() == 2)
    });
    for key in parse_sequence("M-RET").unwrap() {
        editor.handle_key(key);
    }
    wait_until(&mut editor, "the commit", |editor| editor.window_buffer().mode() == "Git Commit");
    bencher.bench_local(|| black_box(editor.text_controls()));
}
