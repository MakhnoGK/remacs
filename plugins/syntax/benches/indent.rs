//! `cargo bench -p remacs-syntax --bench indent`

use std::thread;
use std::time::Duration;

use divan::{Bencher, black_box};
use remacs_core::{Buffer, Editor};
use remacs_syntax::{Grammar, Indents, SyntaxPlugin};
use remacs_text::Text;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const GRAMMARS: &[Grammar] = &[Grammar {
    mode: "Rust",
    language: tree_sitter_rust::LANGUAGE,
    highlights: &[include_str!("../../languages/queries/rust/highlights.scm")],
}];
const INDENTS: &[Indents] = &[Indents::new(
    "Rust",
    &[include_str!("../../languages/queries/rust/indents.scm")],
)];
static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, &[]).with_indents(INDENTS);

/// A real file of the repo.
const SOURCE: &str = include_str!("../../../crates/app/src/editor_view.rs");

fn main() {
    divan::main();
}

/// An editor showing the source repeated `times`, parsed, with point inside a function in
/// its middle.
fn parsed_editor(times: usize) -> Editor {
    let source = SOURCE.repeat(times);
    let mut editor = Editor::new(&[&SYNTAX]);
    editor.add_buffer(Buffer::new("view.rs", Text::from(source.as_str()), "Rust"));
    let middle = SOURCE.len() * (times / 2);
    let point = middle + SOURCE.find("fn render(").unwrap();
    let point = point + source[point..].find(";\n").unwrap() + 1;
    editor.window_buffer_mut().set_point(point);
    for _ in 0..2000 {
        editor.refresh();
        if !editor.decorations(0..200).concat().is_empty() {
            return editor;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("the source was never parsed");
}

/// A line broken, as `RET` does, before the parser thread has caught up.
#[divan::bench(args = [4, 40], sample_count = 30)]
fn break_line(bencher: Bencher, times: usize) {
    let mut editor = parsed_editor(times);
    bencher.bench_local(|| editor.window_buffer_mut().insert("\n"));
}

/// The same, with the new line's indentation told from a tree parsed again.
#[divan::bench(args = [4, 40], sample_count = 30)]
fn break_line_and_indent(bencher: Bencher, times: usize) {
    let mut editor = parsed_editor(times);
    bencher.bench_local(|| {
        let buffer = editor.window_buffer_mut();
        buffer.insert("\n");
        let line = buffer.point_line();
        black_box(editor.indentation_of(editor.window_buffer(), line))
    });
}
