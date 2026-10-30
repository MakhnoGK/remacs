//! `cargo bench -p remacs-xref --bench references`: listing a symbol's references, about
//! as many as `Editor` has in remacs: 1,400 uses in 175 files.

use std::fs;

use divan::{Bencher, black_box};
use remacs_core::{Buffer, Editor};
use remacs_minibuffer::MinibufferPlugin;
use remacs_text::Text;
use remacs_xref::{Location, XrefPlugin, deliver, take_requests};

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const FILES: usize = 175;
const USES_PER_FILE: usize = 8;
const LINES_PER_FILE: usize = 600;

fn main() {
    divan::main();
}

/// Files of about 24 KB on disk, none of them visited.
fn locations() -> Vec<Location> {
    let root = std::env::temp_dir().join(format!("remacs-xref-references-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let line = "    let editor: &mut Editor = context.editor();\n";
    let source = line.repeat(LINES_PER_FILE);
    (0..FILES)
        .flat_map(|index| {
            let path = root.join(format!("file{index}.rs"));
            fs::write(&path, &source).unwrap();
            (0..USES_PER_FILE).map(move |use_index| Location {
                path: path.clone(),
                offset: use_index * 70 * line.len() + 20,
            })
        })
        .collect()
}

#[divan::bench]
fn list_references_in_files_on_disk(bencher: Bencher) {
    let mut editor = Editor::new(&[&MinibufferPlugin, &XrefPlugin]);
    editor.add_buffer(Buffer::new("main.rs", Text::from("editor"), "Rust"));
    let locations = locations();
    bencher.with_inputs(|| locations.clone()).bench_local_values(|locations| {
        editor.cancel_minibuffer();
        editor.run_command("xref-find-references");
        let id = take_requests(&mut editor)[0].id;
        deliver(&mut editor, id, locations);
        black_box(editor.minibuffer().is_some())
    });
}
