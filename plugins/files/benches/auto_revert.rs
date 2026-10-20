//! `cargo bench -p remacs-files --bench auto_revert`

use std::fs;
use std::path::PathBuf;

use divan::{Bencher, black_box};
use remacs_core::Editor;
use remacs_editing::EditingPlugin;
use remacs_files::FilesPlugin;
use remacs_minibuffer::MinibufferPlugin;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

fn main() {
    divan::main();
}

/// A directory of `files` files of `len` bytes each, removed on drop.
struct Files(PathBuf);

impl Files {
    fn new(name: &str, files: usize, len: usize) -> Self {
        let root = std::env::temp_dir().join(format!("remacs-bench-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let line = "fn main() { println!(\"hello, world\"); }\n";
        let contents = line.repeat(len / line.len() + 1);
        for file in 0..files {
            fs::write(root.join(format!("file_{file}.rs")), &contents[..len]).unwrap();
        }
        Self(root.canonicalize().unwrap())
    }

    fn open_all(&self, editor: &mut Editor) {
        for entry in fs::read_dir(&self.0).unwrap() {
            editor.visit_file(&entry.unwrap().path()).unwrap();
        }
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn editor() -> Editor {
    Editor::new(&[&EditingPlugin, &MinibufferPlugin, &FilesPlugin])
}

/// What a background notification costs with `buffers` files open and none changed.
#[divan::bench(args = [100])]
fn refresh_with_files_open(bencher: Bencher, buffers: usize) {
    let files = Files::new("refresh", buffers, 100);
    let mut editor = editor();
    files.open_all(&mut editor);
    editor.refresh();
    bencher.bench_local(|| editor.refresh());
}

/// Reading a 256 MiB file again with `revert-buffer`, as auto-revert does.
#[divan::bench(sample_count = 10)]
fn revert_buffer(bencher: Bencher) {
    let files = Files::new("revert", 1, 256 << 20);
    let mut editor = editor();
    files.open_all(&mut editor);
    bencher.bench_local(|| {
        editor.run_command("revert-buffer");
        black_box(editor.window_buffer().text().len())
    });
}

/// Reading a 256 MiB file again, as `revert-buffer-with-coding-system` does.
#[divan::bench(sample_count = 10)]
fn revert_with_coding_system(bencher: Bencher) {
    let files = Files::new("revert-coding", 1, 256 << 20);
    let mut editor = editor();
    files.open_all(&mut editor);
    bencher.bench_local(|| {
        editor.run_command("revert-buffer-with-coding-system");
        editor.minibuffer_mut().unwrap().set_input("UTF-8");
        editor.exit_minibuffer(true).unwrap();
        black_box(editor.window_buffer().text().len())
    });
}
