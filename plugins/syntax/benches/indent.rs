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
    highlights: &[
        include_str!("../../languages/queries/rust/highlights.scm"),
        include_str!("../../languages/queries/rust/text.scm"),
    ],
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

/// Every block of the source hidden, as `hs-hide-all` does.
#[divan::bench(args = [4, 40], sample_count = 10)]
fn hide_all_blocks(bencher: Bencher, times: usize) {
    let mut editor = parsed_editor(times);
    let line_count = editor.window_buffer().text().line_count();
    bencher.bench_local(|| {
        let blocks = editor
            .blocks(editor.window_buffer(), 0..line_count)
            .unwrap();
        let buffer = editor.window_buffer_mut();
        buffer.fold(blocks.into_iter().map(|block| block.hidden));
        buffer.unfold_all();
    });
}

/// What a frame asks to mark the blocks that start on the 60 lines it shows.
#[divan::bench(args = [4, 40], sample_count = 30)]
fn blocks_of_a_screen(bencher: Bencher, times: usize) {
    let editor = parsed_editor(times);
    let first = editor.window_buffer().point_line();
    let buffer = editor.window_buffer();
    bencher.bench_local(|| black_box(editor.blocks(buffer, first..first + 60)));
}

/// The same of a list of 200 000 items, by its brackets, without an indent query.
#[divan::bench(sample_count = 30)]
fn blocks_of_a_screen_in_a_long_list(bencher: Bencher) {
    static BRACKETS: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, &[]);
    let source = format!(
        "const ITEMS: &[u32] = &[\n{}];\n",
        "    1,\n".repeat(200_000)
    );
    let mut editor = Editor::new(&[&BRACKETS]);
    editor.add_buffer(Buffer::new("items.rs", Text::from(source.as_str()), "Rust"));
    for _ in 0..4000 {
        editor.refresh();
        if editor.blocks(editor.window_buffer(), 0..1).is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    let buffer = editor.window_buffer();
    bencher.bench_local(|| black_box(editor.blocks(buffer, 100_000..100_060)));
}

/// What a frame asks of the folds, again and again, with every block hidden.
#[divan::bench(args = [4, 40], sample_count = 30)]
fn shown_lines_all_hidden(bencher: Bencher, times: usize) {
    let mut editor = parsed_editor(times);
    let line_count = editor.window_buffer().text().line_count();
    let blocks = editor
        .blocks(editor.window_buffer(), 0..line_count)
        .unwrap();
    editor
        .window_buffer_mut()
        .fold(blocks.into_iter().map(|block| block.hidden));
    bencher.bench_local(|| black_box(editor.window_buffer().shown_lines()));
}

/// A character typed at the start with every block hidden, which each fold follows.
#[divan::bench(args = [4, 40], sample_count = 30)]
fn type_with_every_block_hidden(bencher: Bencher, times: usize) {
    let mut editor = parsed_editor(times);
    let line_count = editor.window_buffer().text().line_count();
    let blocks = (editor.blocks(editor.window_buffer(), 0..line_count)).unwrap();
    let buffer = editor.window_buffer_mut();
    buffer.fold(blocks.into_iter().map(|block| block.hidden));
    buffer.set_point(0);
    bencher.bench_local(|| buffer.insert("x"));
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
