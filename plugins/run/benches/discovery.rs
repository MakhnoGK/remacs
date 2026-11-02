//! `cargo bench -p remacs-run --bench discovery`

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use divan::{Bencher, black_box};
use remacs_core::{Editor, Key};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;
use remacs_run::discovery::discover;
use remacs_run::{RunPlugin, choose_with};

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

fn main() {
    divan::main();
}

/// A monorepo of `packages` npm packages of five scripts each and as many binaries of
/// one Cargo workspace, removed on drop.
struct Monorepo {
    root: PathBuf,
    files: Vec<String>,
}

impl Monorepo {
    fn new(packages: usize) -> Self {
        let root = std::env::temp_dir()
            .join(format!("remacs-bench-discovery-{}-{packages}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut files = Vec::new();
        let mut write = |file: String, text: &str| {
            let path = root.join(&file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
            files.push(file);
        };
        write("Cargo.toml".into(), "[workspace]\nmembers = [\"crates/*\"]\n");
        let scripts = "{\n  \"scripts\": {\n    \"dev\": \"vite\",\n    \"build\": \"vite build\",\n    \"test\": \"vitest\",\n    \"lint\": \"oxlint\",\n    \"format\": \"oxfmt\"\n  }\n}\n";
        for index in 0..packages {
            write(format!("packages/p{index}/package.json"), scripts);
            write(format!("packages/p{index}/src/index.ts"), "export {};\n");
            let manifest = format!("[package]\nname = \"c{index}\"\n");
            write(format!("crates/c{index}/Cargo.toml"), &manifest);
            write(format!("crates/c{index}/src/main.rs"), "fn main() {}\n");
        }
        Self { root: root.canonicalize().unwrap(), files }
    }
}

impl Drop for Monorepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// What opening the run chooser spends finding what the project can run.
#[divan::bench(args = [100, 1000], sample_count = 10)]
fn discover_in_monorepo(bencher: Bencher, packages: usize) {
    let monorepo = Monorepo::new(packages);
    bencher.bench_local(|| black_box(discover(&monorepo.root, &monorepo.files).len()));
}

fn candidates(editor: &Editor) -> usize {
    editor.minibuffer().map_or(0, |minibuffer| minibuffer.candidates().len())
}

/// 5 scripts and a binary per package, and "New configuration…".
const LISTED: usize = 5 * 1000 + 1000 + 1;

/// An editor in a monorepo of 1000 packages with the run chooser open, everything the
/// project can run listed.
fn open_chooser(monorepo: &Monorepo) -> Editor {
    fs::create_dir_all(monorepo.root.join(".git")).unwrap();
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &RunPlugin]);
    editor.set_settings(Default::default());
    editor.visit_file(&monorepo.root.join("Cargo.toml")).unwrap();
    editor.refresh();
    choose_with(&mut editor, None).unwrap();
    while candidates(&editor) < LISTED {
        thread::sleep(Duration::from_millis(10));
        editor.refresh();
    }
    editor
}

/// What the main thread spends opening the run chooser again, the project searched
/// before: the command, then the refresh that draws every row.
#[divan::bench(sample_count = 10)]
fn open_chooser_in_monorepo(bencher: Bencher) {
    let monorepo = Monorepo::new(1000);
    let mut editor = open_chooser(&monorepo);
    let cancel = Key::parse("C-g").unwrap();
    editor.handle_key(cancel);
    bencher.bench_local(|| {
        choose_with(&mut editor, None).unwrap();
        editor.refresh();
        editor.handle_key(cancel);
    });
}

/// What the main thread spends on a key typed into the open chooser.
#[divan::bench(sample_count = 10)]
fn type_in_chooser_in_monorepo(bencher: Bencher) {
    let monorepo = Monorepo::new(1000);
    let mut editor = open_chooser(&monorepo);
    let (letter, delete) = (Key::parse("d").unwrap(), Key::parse("DEL").unwrap());
    bencher.bench_local(|| {
        editor.handle_key(letter);
        editor.refresh();
        editor.handle_key(delete);
        editor.refresh();
    });
}
