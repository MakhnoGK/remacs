use std::thread;
use std::time::{Duration, Instant};

use remacs_core::Editor;

use crate::indent_tests::editor_with;
use crate::modes::*;

/// An editor with `text`, its syntax tree parsed.
fn parsed(name: &str, mode: &'static str, text: &str) -> Editor {
    let mut editor = editor_with(name, mode, text);
    let start = Instant::now();
    while editor.blocks(editor.buffer(), 0..1).is_none() {
        assert!(start.elapsed() < Duration::from_secs(5), "never parsed");
        thread::sleep(Duration::from_millis(5));
        editor.refresh();
    }
    editor
}

/// The lines the window shows.
fn shown(editor: &Editor) -> Vec<String> {
    let buffer = editor.buffer();
    let shown = buffer.shown_lines();
    (0..shown.rows())
        .map(|row| buffer.contents(buffer.text().line_range(shown.line_at(row)).unwrap()))
        .collect()
}

#[test]
fn a_block_hides_its_lines_but_its_first_and_its_closer() {
    let text = "fn f() {\n    if a {\n        b();\n    }\n}\nfn g() {}\n";
    let mut editor = parsed("main.rs", RUST, text);
    editor.buffer_mut().set_point(text.find("b()").unwrap());
    editor.run_command("hs-toggle-hiding");
    let hidden_if = ["fn f() {", "    if a {", "    }", "}", "fn g() {}", ""];
    assert_eq!(shown(&editor), hidden_if);
    assert_eq!(
        editor.buffer().point_line(),
        1,
        "point goes to the block's first line"
    );
    editor.run_command("hs-toggle-hiding");
    assert_eq!(shown(&editor).len(), 7, "shown again from its first line");

    editor.run_command("hs-hide-all");
    assert_eq!(shown(&editor), ["fn f() {", "}", "fn g() {}", ""]);
    editor.buffer_mut().set_point(0);
    editor.run_command("next-line");
    assert_eq!(
        editor.buffer().point_line(),
        4,
        "C-n goes over the hidden lines"
    );
    editor.buffer_mut().set_point(0);
    editor.run_command("hs-toggle-hiding");
    assert_eq!(shown(&editor), hidden_if, "the blocks within stay hidden");
    editor.run_command("hs-show-all");
    assert_eq!(shown(&editor).len(), 7);
}

#[test]
fn a_python_block_hides_to_its_last_line_and_shows_when_point_goes_in() {
    let text = "def f():\n    a = 1\n    return a\n\nx = 2\n";
    let mut editor = parsed("main.py", PYTHON, text);
    editor.run_command("hs-toggle-hiding");
    assert_eq!(shown(&editor), ["def f():", "", "x = 2", ""]);
    editor.buffer_mut().set_point(text.find("return").unwrap());
    editor.refresh();
    assert_eq!(shown(&editor).len(), 6, "point in a fold shows it");
}

#[test]
fn an_edit_in_a_hidden_block_shows_it() {
    let text = "{\n  \"a\": [\n    1\n  ]\n}\n";
    let mut editor = parsed("a.json", JSON, text);
    editor.run_command("hs-toggle-hiding");
    assert_eq!(shown(&editor), ["{", "}", ""]);
    let inside = text.find('1').unwrap();
    editor
        .buffer_mut()
        .apply_edits(vec![(inside..inside + 1, "2".into())]);
    editor.refresh();
    assert_eq!(shown(&editor).len(), 6);
}

#[test]
fn a_function_in_a_pages_script_is_a_block() {
    let text = "<script>\nfunction f() {\n  a();\n}\n</script>\n";
    let mut editor = parsed("page.html", HTML, text);
    editor.buffer_mut().set_point(text.find("a()").unwrap());
    editor.run_command("hs-toggle-hiding");
    assert_eq!(
        shown(&editor),
        ["<script>", "function f() {", "}", "</script>", ""]
    );
}

/// Without an indent query, the lists between brackets are the blocks.
#[test]
fn brackets_make_the_blocks_of_other_languages() {
    let text = "fun f() {\n    a()\n}\n";
    let mut editor = parsed("main.kt", KOTLIN, text);
    editor.run_command("hs-toggle-hiding");
    assert_eq!(shown(&editor), ["fun f() {", "}", ""]);
}
