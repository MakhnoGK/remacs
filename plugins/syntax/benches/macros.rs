//! `cargo bench -p remacs-syntax --bench macros`: a test file of the repo, about 900
//! macros, repeated to 8 000 lines, with and without the code in macros' arguments
//! highlighted.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use divan::{Bencher, black_box};
use remacs_core::{Buffer, Editor};
use remacs_syntax::{Grammar, Injections, Source, SyntaxPlugin};
use remacs_text::Text;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const CODE: &str = include_str!("../../languages/queries/rust/highlights.scm");
const GRAMMARS: &[Grammar] = &[
    Grammar {
        mode: "Rust",
        language: Source::Built(tree_sitter_rust::LANGUAGE),
        highlights: &[CODE, include_str!("../../languages/queries/rust/text.scm")],
    },
    Grammar {
        mode: "rust_macro_arguments",
        language: Source::Built(tree_sitter_rust::LANGUAGE),
        highlights: &[CODE],
    },
];
const MACRO_INJECTIONS: &str = include_str!("../../languages/queries/rust/injections.scm");
const INJECTIONS: &[Injections] = &[
    Injections {
        host: "Rust",
        query: MACRO_INJECTIONS,
        documents: false,
        servers: false,
    },
    Injections {
        host: "rust_macro_arguments",
        query: MACRO_INJECTIONS,
        documents: false,
        servers: false,
    },
];
static WITHOUT_MACROS: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, &[]);
static WITH_MACROS: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS);

const SOURCE: &str = include_str!("../../git/src/tests.rs");
const VISIBLE_LINES: usize = 60;

fn main() {
    divan::main();
}

/// An editor showing the source parsed, point in its middle, and a channel the parser
/// thread's notifications arrive on.
fn parsed_editor(syntax: &'static SyntaxPlugin) -> (Editor, mpsc::Receiver<()>) {
    let source = SOURCE.repeat(5);
    let mut editor = Editor::new(&[syntax]);
    let (notify, notified) = mpsc::channel();
    let notify = Mutex::new(notify);
    editor.set_notifier(Arc::new(move || {
        let _ = notify.lock().unwrap().send(());
    }));
    editor.add_buffer(Buffer::new("tests.rs", Text::from(source.as_str()), "Rust"));
    let middle = SOURCE.len() * 2 + SOURCE.find("assert_eq!(").unwrap();
    editor.window_buffer_mut().set_point(middle);
    for _ in 0..2000 {
        editor.refresh();
        if !editor.decorations(0..200).concat().is_empty() {
            while notified.try_recv().is_ok() {}
            return (editor, notified);
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("the source was never parsed");
}

/// Typing a character in a macro's arguments, until the parser thread has the new tree.
fn edit_and_reparse(bencher: Bencher, syntax: &'static SyntaxPlugin) {
    let (mut editor, notified) = parsed_editor(syntax);
    bencher.bench_local(|| {
        editor.window_buffer_mut().insert("x");
        editor.refresh();
        notified.recv().unwrap();
        editor.refresh();
    });
}

/// What a frame asks of the decorators for the visible lines.
fn visible_lines(bencher: Bencher, syntax: &'static SyntaxPlugin) {
    let (editor, _notified) = parsed_editor(syntax);
    let text = editor.window_buffer().text();
    let first_line = text.line_of(editor.window_buffer().point());
    let lines: Vec<_> = (first_line..first_line + VISIBLE_LINES)
        .map(|line| text.line_range(line).unwrap())
        .collect();
    bencher.bench_local(|| black_box(editor.decorations_by_line(&lines)));
}

#[divan::bench(sample_count = 50)]
fn edit_and_reparse_without_macros(bencher: Bencher) {
    edit_and_reparse(bencher, &WITHOUT_MACROS);
}

#[divan::bench(sample_count = 50)]
fn edit_and_reparse_with_macros(bencher: Bencher) {
    edit_and_reparse(bencher, &WITH_MACROS);
}

#[divan::bench(sample_count = 50)]
fn visible_lines_without_macros(bencher: Bencher) {
    visible_lines(bencher, &WITHOUT_MACROS);
}

#[divan::bench(sample_count = 50)]
fn visible_lines_with_macros(bencher: Bencher) {
    visible_lines(bencher, &WITH_MACROS);
}
