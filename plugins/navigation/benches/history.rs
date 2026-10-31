//! `cargo bench -p remacs-navigation --bench history`
//!
//! The navigation history checks where point is on every refresh, which background work
//! (program output, language servers) asks for many times a second.

use std::path::PathBuf;

use divan::Bencher;
use remacs_core::{Buffer, Editor};
use remacs_editing::EditingPlugin;
use remacs_navigation::NavigationPlugin;
use remacs_text::Text;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

fn main() {
    divan::main();
}

#[divan::bench]
fn refresh_with_point_still(bencher: Bencher) {
    let mut editor = Editor::new(&[&EditingPlugin, &NavigationPlugin]);
    let path = PathBuf::from("/project/src/a_rather_long_module_name.rs");
    let text: String = (0..1000).map(|line| format!("line {line}\n")).collect();
    let buffer = Buffer::new("a_rather_long_module_name.rs", Text::from(text.as_str()), "Rust");
    editor.add_buffer(buffer.with_file(path));
    editor.refresh();
    bencher.bench_local(|| editor.refresh());
}
