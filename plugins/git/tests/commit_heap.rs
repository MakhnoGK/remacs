//! A test binary of its own: dhat replaces the global allocator.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{Editor, OutlineRow, parse_sequence};
use remacs_dired::DiredPlugin;
use remacs_editing::EditingPlugin;
use remacs_files::FilesPlugin;
use remacs_git::GitPlugin;
use remacs_minibuffer::MinibufferPlugin;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// 50 MB of lockfile lines.
const LOCK_LINES: usize = 1_000_000;
const TIMEOUT: Duration = Duration::from_secs(120);

/// A throwaway repository whose last commit adds a 50 MB lockfile and a small file.
struct Repository(PathBuf);

impl Repository {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("remacs-git-commit-heap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let repository = Self(root.canonicalize().unwrap());
        repository.git(&["init", "-q", "-b", "main"]);
        repository.git(&["config", "user.email", "test@example.com"]);
        repository.git(&["config", "user.name", "Test"]);
        fs::write(repository.0.join("lib.rs"), "one\n").unwrap();
        repository.git(&["add", "."]);
        repository.git(&["commit", "-q", "-m", "First"]);
        let lock: String = (0..LOCK_LINES)
            .map(|line| format!("dependency-{line:08} = \"1.0.{line:08}\" # pinned\n"))
            .collect();
        fs::write(repository.0.join("Cargo.lock"), lock).unwrap();
        fs::write(repository.0.join("z.rs"), "small\n").unwrap();
        repository.git(&["add", "."]);
        repository.git(&["commit", "-q", "-m", "Vendor"]);
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

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

#[test]
fn a_commit_with_a_50_mb_patch_opens_without_holding_the_ui_and_its_diff_capped() {
    let repository = Repository::new();
    let mut editor = Editor::new(&[
        &EditingPlugin,
        &MinibufferPlugin,
        &FilesPlugin,
        &DiredPlugin,
        &GitPlugin,
    ]);
    editor.viewport.set_height(20);
    editor.visit_file(&repository.0.join("lib.rs")).unwrap();
    editor.run_command("git-status");
    editor.run_command("git-status-history");
    wait_until(&mut editor, "the history", |editor| {
        editor
            .outline(0..100)
            .and_then(|outline| outline.history)
            .is_some_and(|history| history.commits.len() == 2)
    });

    let _profiler = dhat::Profiler::builder().testing().build();
    let start = Instant::now();
    press(&mut editor, "M-RET");
    let held = start.elapsed();
    wait_until(&mut editor, "the commit", |editor| {
        editor.window_buffer().mode() == "Git Commit"
    });
    let shown = start.elapsed();
    let peak = dhat::HeapStats::get().max_bytes;
    eprintln!("UI held {held:?}, commit shown after {shown:?}, peak heap {peak} B");

    let side = editor.text_controls().unwrap().side.unwrap();
    let files: Vec<&str> = side
        .rows
        .iter()
        .filter_map(|row| match row {
            OutlineRow::File { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(files, ["Cargo.lock", "z.rs"]);
    let text = editor.window_buffer().text().snapshot();
    let diff = String::from_utf8_lossy(&text.bytes_in(0..text.len())).into_owned();
    assert!(diff.contains("not shown, past"), "{diff}");
    assert!(diff.ends_with("\n+small"), "{diff}");
    // Read on the UI thread, uncapped, it held the UI 14.7 s and peaked at 138 MB.
    assert!(held < Duration::from_millis(100), "UI held {held:?}");
    dhat::assert!(peak < 32 << 20);
}
