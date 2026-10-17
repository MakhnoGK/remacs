//! `cargo bench -p remacs-syntax --bench html`: a page of about 600 KB and 240k nodes
//! with `<style>` and `<script>` blocks, the code in them highlighted as its own language.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use divan::{Bencher, black_box};
use remacs_core::{Buffer, Editor};
use remacs_syntax::{Grammar, Injections, SyntaxPlugin};
use remacs_text::Text;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const GRAMMARS: &[Grammar] = &[
    Grammar {
        mode: "HTML",
        language: tree_sitter_html::LANGUAGE,
        highlights: &[tree_sitter_html::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: "JavaScript",
        language: tree_sitter_javascript::LANGUAGE,
        highlights: &[tree_sitter_javascript::HIGHLIGHT_QUERY],
    },
    Grammar {
        mode: "CSS",
        language: tree_sitter_css::LANGUAGE,
        highlights: &[tree_sitter_css::HIGHLIGHTS_QUERY],
    },
];
const INJECTIONS: &[Injections] = &[Injections {
    host: "HTML",
    query: tree_sitter_html::INJECTIONS_QUERY,
}];
static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS);

const SECTIONS: usize = 3000;
/// Sections between a `<style>` and a `<script>` block.
const BLOCK_EVERY: usize = 20;
const VISIBLE_LINES: usize = 60;
/// Where the edits go and the visible lines start: in the middle of the page.
const MARKER: &str = "function render1500(";

fn main() {
    divan::main();
}

fn page() -> String {
    let mut page = String::from("<!doctype html>\n<html>\n<body>\n");
    for section in 0..SECTIONS {
        page.push_str(&format!(
            "<section class=\"card\" id=\"card-{section}\">\n  <h2 title=\"Card {section}\">Card {section}</h2>\n  <p>Some <b>text</b> about card {section}, <a href=\"/cards/{section}\">more</a>.</p>\n</section>\n"
        ));
        if section % BLOCK_EVERY == 0 {
            page.push_str(&format!(
                "<style>\n.card-{section} {{\n  display: flex;\n  margin: 0 auto;\n  color: #336699;\n}}\n.card-{section}:hover > h2 {{ text-decoration: underline; }}\n</style>\n"
            ));
            page.push_str(&format!(
                "<script>\nfunction render{section}(cards) {{\n  const list = document.querySelector(\"#card-{section}\");\n  for (const card of cards) {{\n    if (card.visible) {{\n      list.append(`${{card.title}}: ${{card.count * 2}}`);\n    }}\n  }}\n  return list;\n}}\n</script>\n"
            ));
        }
    }
    page.push_str("</body>\n</html>\n");
    page
}

/// An editor showing the page parsed, point in a script in the middle, and a channel the
/// parser thread's notifications arrive on.
fn parsed_editor() -> (Editor, mpsc::Receiver<()>) {
    let source = page();
    let mut editor = Editor::new(&[&SYNTAX]);
    let (notify, notified) = mpsc::channel();
    let notify = Mutex::new(notify);
    editor.set_notifier(Arc::new(move || {
        let _ = notify.lock().unwrap().send(());
    }));
    editor.add_buffer(Buffer::new(
        "page.html",
        Text::from(source.as_str()),
        "HTML",
    ));
    let marker = source.find(MARKER).unwrap();
    editor.window_buffer_mut().set_point(marker);
    for _ in 0..2000 {
        editor.refresh();
        if !editor.decorations(0..200).concat().is_empty() {
            while notified.try_recv().is_ok() {}
            return (editor, notified);
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("the page was never parsed");
}

/// Typing a character in a script, until the parser thread has the new tree.
#[divan::bench(sample_count = 50)]
fn edit_and_reparse(bencher: Bencher) {
    let (mut editor, notified) = parsed_editor();
    bencher.bench_local(|| {
        editor.window_buffer_mut().insert("x");
        editor.refresh();
        notified.recv().unwrap();
        editor.refresh();
    });
}

/// What a frame asks of the decorators for the lines of a script.
#[divan::bench(sample_count = 50)]
fn visible_lines_in_a_script(bencher: Bencher) {
    let (editor, _notified) = parsed_editor();
    let text = editor.window_buffer().text();
    let first_line = text.line_of(editor.window_buffer().point());
    let lines: Vec<_> = (first_line..first_line + VISIBLE_LINES)
        .map(|line| text.line_range(line).unwrap())
        .collect();
    bencher.bench_local(|| black_box(editor.decorations_by_line(&lines)));
}
