//! `cargo bench -p remacs-syntax --bench decorations`

use std::thread;
use std::time::Duration;

use divan::{Bencher, black_box};
use remacs_core::{Buffer, Editor};
use remacs_syntax::{Grammar, SyntaxPlugin};
use remacs_text::Text;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const GRAMMARS: &[Grammar] = &[Grammar {
    mode: "Rust",
    language: tree_sitter_rust::LANGUAGE,
    highlights: &[tree_sitter_rust::HIGHLIGHTS_QUERY],
}];
static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, &[]);

/// A real file of the repo, repeated to about 9 000 lines.
const SOURCE: &str = include_str!("../../../crates/app/src/editor_view.rs");
const VISIBLE_LINES: usize = 60;

fn main() {
    divan::main();
}

/// An editor showing the source parsed, with point on the `{` that opens `impl Render`,
/// so the matching pair is looked for too.
fn parsed_editor() -> (Editor, usize) {
    let source = SOURCE.repeat(4);
    let mut editor = Editor::new(&[&SYNTAX]);
    editor.add_buffer(Buffer::new("view.rs", Text::from(source.as_str()), "Rust"));
    let brace = SOURCE.find("impl Render for EditorView {").unwrap() + SOURCE.len() * 2;
    let brace = brace + source[brace..].find('{').unwrap();
    editor.window_buffer_mut().set_point(brace);
    for _ in 0..2000 {
        editor.refresh();
        if !editor.decorations(0..200).concat().is_empty() {
            let first_line = editor.window_buffer().text().line_of(brace);
            return (editor, first_line);
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("the source was never parsed");
}

fn visible(editor: &Editor, first_line: usize) -> Vec<std::ops::Range<usize>> {
    let text = editor.window_buffer().text();
    (first_line..first_line + VISIBLE_LINES)
        .map(|line| text.line_range(line).unwrap())
        .collect()
}

/// The decorators asked one line at a time, as frames did before.
#[divan::bench(sample_count = 50)]
fn visible_lines_one_at_a_time(bencher: Bencher) {
    let (editor, first_line) = parsed_editor();
    let lines = visible(&editor, first_line);
    bencher.bench_local(|| {
        for line in &lines {
            black_box(editor.decorations(line.clone()));
        }
    });
}

/// What a commit's diff asks once per side of a file, as the file is first drawn: 2000
/// lines, the most it shows.
#[divan::bench(sample_count = 20)]
fn diff_side(bencher: Bencher) {
    let editor = Editor::new(&[&SYNTAX]);
    let code: String = SOURCE
        .lines()
        .take(2000)
        .map(|line| format!("{line}\n"))
        .collect();
    bencher.bench_local(|| black_box(editor.highlight_code("Rust", &code)));
}

/// What a frame asks of the decorators for the visible lines.
#[divan::bench(sample_count = 50)]
fn visible_lines(bencher: Bencher) {
    let (editor, first_line) = parsed_editor();
    let lines = visible(&editor, first_line);
    bencher.bench_local(|| black_box(editor.decorations_by_line(&lines)));
}
