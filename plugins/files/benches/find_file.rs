//! `cargo bench -p remacs-files --bench find_file`

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use divan::{Bencher, black_box};
use remacs_core::{Editor, Key, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_files::FilesPlugin;
use remacs_minibuffer::MinibufferPlugin;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const DIRECTORIES: usize = 200;

fn main() {
    divan::main();
}

/// A Git work tree of `files` empty files, removed on drop.
struct Project(PathBuf);

impl Project {
    fn new(files: usize) -> Self {
        let root =
            std::env::temp_dir().join(format!("remacs-bench-{}-{files}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git/HEAD"), "").unwrap();
        for file in 0..files {
            let directory = root.join(format!("src/module_{}", file % DIRECTORIES));
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join(format!("source_file_{file}.rs")), "").unwrap();
        }
        Self(root.canonicalize().unwrap())
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// What a background notification costs while find-file is open on `files` with the
/// input unchanged: the terminal, a language server or git woke the editor.
#[divan::bench(args = [20_000], sample_count = 30)]
fn refresh_with_find_file_open(bencher: Bencher, files: usize) {
    let project = Project::new(files);
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &FilesPlugin]);
    editor.visit_file(&project.0.join("src/module_0/source_file_0.rs")).unwrap();
    let (sender, notifications) = mpsc::channel();
    editor.set_notifier(Arc::new(move || {
        let _ = sender.send(());
    }));
    for key in parse_sequence("C-x C-f").unwrap() {
        editor.handle_key(key);
    }
    notifications.recv_timeout(Duration::from_secs(30)).expect("the index finishes its first scan");
    for character in "srcfile1".chars() {
        editor.handle_key(Key::parse(&character.to_string()).unwrap());
    }
    editor.refresh();
    assert!(!editor.minibuffer().unwrap().candidates().is_empty());
    bencher.bench_local(|| {
        editor.refresh();
        black_box(editor.minibuffer().unwrap().candidates().len())
    });
}
