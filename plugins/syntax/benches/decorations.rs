//! `cargo bench -p remacs-syntax --bench decorations`

use std::thread;
use std::time::Duration;

use divan::{Bencher, black_box};
use remacs_core::{Buffer, Editor};
use remacs_syntax::{Grammar, Locals, SyntaxPlugin};
use remacs_text::Text;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const GRAMMARS: &[Grammar] = &[Grammar {
    mode: "Rust",
    language: tree_sitter_rust::LANGUAGE,
    highlights: &[include_str!("../../languages/queries/rust/highlights.scm")],
}];
static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, &[]);
const LOCALS: &[Locals] = &[Locals {
    mode: "Rust",
    queries: &[include_str!("../../languages/queries/rust/locals.scm")],
}];
static SYNTAX_WITH_LOCALS: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, &[]).with_locals(LOCALS);

/// A real file of the repo, repeated to about 9 000 lines.
const SOURCE: &str = include_str!("../../../crates/app/src/editor_view.rs");
const VISIBLE_LINES: usize = 60;

fn main() {
    divan::main();
}

/// An editor showing the source parsed, with point on the `{` that opens `impl Render`,
/// so the matching pair is looked for too.
fn parsed_editor() -> (Editor, usize) {
    parsed_editor_with(&SYNTAX)
}

fn parsed_editor_with(syntax: &'static SyntaxPlugin) -> (Editor, usize) {
    let source = SOURCE.repeat(4);
    let mut editor = Editor::new(&[syntax]);
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

/// What a frame asks with a block of 500 lines folded in the middle of the window.
#[divan::bench(sample_count = 50)]
fn visible_lines_around_a_fold(bencher: Bencher) {
    let (editor, first_line) = parsed_editor_with(&SYNTAX_WITH_LOCALS);
    let mut lines = visible(&editor, first_line.saturating_sub(500));
    let after = visible(&editor, first_line);
    lines.truncate(VISIBLE_LINES / 2);
    lines.extend(after.into_iter().take(VISIBLE_LINES / 2));
    bencher.bench_local(|| black_box(editor.decorations_by_line(&lines)));
}

/// The same, with the functions' locals highlighted.
#[divan::bench(sample_count = 50)]
fn visible_lines_with_locals(bencher: Bencher) {
    let (editor, first_line) = parsed_editor_with(&SYNTAX_WITH_LOCALS);
    let lines = visible(&editor, first_line);
    bencher.bench_local(|| black_box(editor.decorations_by_line(&lines)));
}

/// A frame after a key typed in one function of 1 MB, like a bundled file wrapped in one,
/// whose definitions are read anew.
#[divan::bench(sample_count = 30)]
fn typing_with_locals_in_one_big_function(bencher: Bencher) {
    let source = format!("fn all() {{\n{}}}\n", SOURCE.repeat(40));
    let mut editor = Editor::new(&[&SYNTAX_WITH_LOCALS]);
    editor.add_buffer(Buffer::new("all.rs", Text::from(source.as_str()), "Rust"));
    let middle = source.len() / 2;
    editor.window_buffer_mut().set_point(middle);
    for _ in 0..4000 {
        editor.refresh();
        if !editor.decorations(0..200).concat().is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    let first_line = editor.window_buffer().text().line_of(middle);
    let lines = visible(&editor, first_line);
    let typed_at = lines[0].start;
    bencher.bench_local(|| {
        editor.window_buffer_mut().set_point(typed_at);
        editor.window_buffer_mut().insert(" ");
        editor.refresh();
        black_box(editor.decorations_by_line(&lines))
    });
}

/// The same, each frame elsewhere, so the functions' definitions are read anew, as after
/// an edit.
#[divan::bench(sample_count = 50)]
fn visible_lines_with_locals_read_anew(bencher: Bencher) {
    let (editor, first_line) = parsed_editor_with(&SYNTAX_WITH_LOCALS);
    let elsewhere = [
        visible(&editor, first_line),
        visible(&editor, first_line + SOURCE.lines().count()),
    ];
    let mut turn = 0;
    bencher.bench_local(|| {
        turn += 1;
        black_box(editor.decorations_by_line(&elsewhere[turn % 2]))
    });
}
