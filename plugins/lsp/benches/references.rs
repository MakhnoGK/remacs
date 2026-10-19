//! `cargo bench -p remacs-lsp --bench references`: turning a server's references into
//! buffer offsets, about as many as `Editor` has in remacs: 1,400 uses in 175 files.

use std::fs;
use std::path::PathBuf;

use divan::{Bencher, black_box};
use lsp_types::Position;
use remacs_core::Editor;
use remacs_lsp::definition::locations;
use remacs_lsp::position::Encoding;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const FILES: usize = 175;
const USES_PER_FILE: u32 = 8;
const LINES_PER_FILE: usize = 600;

fn main() {
    divan::main();
}

/// Files of about 24 KB on disk, none of them visited.
fn targets() -> Vec<(PathBuf, Position)> {
    let root = std::env::temp_dir().join(format!("remacs-lsp-references-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = "    let editor: &mut Editor = context.editor();\n".repeat(LINES_PER_FILE);
    (0..FILES)
        .flat_map(|index| {
            let path = root.join(format!("file{index}.rs"));
            fs::write(&path, &source).unwrap();
            (0..USES_PER_FILE)
                .map(move |use_index| (path.clone(), Position::new(use_index * 70, 20)))
        })
        .collect()
}

#[divan::bench]
fn references_in_files_on_disk(bencher: Bencher) {
    let editor = Editor::new(&[]);
    let targets = targets();
    bencher
        .with_inputs(|| targets.clone())
        .bench_local_values(|targets| black_box(locations(&editor, targets, Encoding::Utf16)));
}
