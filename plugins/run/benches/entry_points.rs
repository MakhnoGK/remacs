//! `cargo bench -p remacs-run --bench entry_points`

use std::fs;
use std::path::PathBuf;

use divan::{Bencher, black_box};
use remacs_core::{Editor, Key, Mode, Plugin, Registry};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;
use remacs_run::RunPlugin;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

/// A real file of the repo with tests, repeated to about 9 000 lines.
const SOURCE: &str = include_str!("../../../crates/app/src/editor_view.rs");
const TESTS: &str = "\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn works() {}\n}\n";

fn main() {
    divan::main();
}

struct RustMode;

impl Plugin for RustMode {
    fn register(&self, registry: &mut Registry) {
        registry.modes.add(Mode {
            name: "Rust",
            extensions: &["rs"],
            ..Mode::default()
        });
    }
}

/// A Cargo package with one big library file, removed on drop.
struct Package(PathBuf);

impl Package {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("remacs-bench-run-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"big\"\n").unwrap();
        fs::write(root.join("src/lib.rs"), SOURCE.repeat(4) + TESTS).unwrap();
        Self(root.canonicalize().unwrap())
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// What the main thread spends when typing pauses in a big Rust file: one key, then the
/// refresh that finds the entry points again.
#[divan::bench(sample_count = 30)]
fn key_then_pause(bencher: Bencher) {
    let package = Package::new();
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &RustMode, &RunPlugin]);
    editor.set_settings(Default::default());
    editor.visit_file(&package.0.join("src/lib.rs")).unwrap();
    editor.refresh();
    let space = Key::parse(" ").unwrap();
    bencher.bench_local(|| {
        editor.handle_key(space);
        editor.refresh();
        black_box(editor.gutter_marks(0..10))
    });
}
