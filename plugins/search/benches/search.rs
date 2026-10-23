//! `cargo bench -p remacs-search --bench search`

use divan::{Bencher, black_box};
use remacs_core::{Buffer, Editor, Key, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;
use remacs_search::SearchPlugin;
use remacs_text::Text;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

fn main() {
    divan::main();
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn type_text(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        editor.handle_key(Key::parse(&character.to_string()).unwrap());
    }
}

/// C-s open on `lines` lines of code, `typed` already in the input.
fn searching(lines: usize, typed: &str) -> Editor {
    let source: String = (0..lines)
        .map(|line| format!("    let value_{line} = compute(item, \"some text here\");\n"))
        .collect();
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &SearchPlugin]);
    editor.add_buffer(Buffer::new("big.rs", Text::from(source.as_str()), "Rust"));
    press(&mut editor, "C-s");
    type_text(&mut editor, typed);
    editor
}

/// One key typed and taken back in a search for a string found once, near the end.
#[divan::bench(args = [1_000_000], sample_count = 20)]
fn key_in_search_for_rare_string(bencher: Bencher, lines: usize) {
    let mut editor = searching(lines, &format!("value_{}", lines - 10));
    bencher.bench_local(|| {
        type_text(&mut editor, " ");
        press(&mut editor, "DEL");
        black_box(editor.minibuffer().unwrap().candidates().len())
    });
}

/// A buffer of one line of `bytes` bytes with a number every 7, like minified code.
fn one_long_line(bytes: usize) -> Editor {
    let line: String = (0..bytes / 7)
        .map(|n| format!("w{:04} ", n % 10_000))
        .collect();
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &SearchPlugin]);
    editor.add_buffer(Buffer::new("min.js", Text::from(line.as_str()), "Rust"));
    editor
}

/// `!` in `query-replace-regexp` of a group on one long line, a match every 7 bytes.
#[divan::bench(args = [200_000, 400_000], sample_count = 5)]
fn replace_all_regexp_on_a_long_line(bencher: Bencher, bytes: usize) {
    bencher
        .with_inputs(|| {
            let mut editor = one_long_line(bytes);
            press(&mut editor, "C-M-%");
            type_text(&mut editor, r"(\d+)");
            press(&mut editor, "RET");
            type_text(&mut editor, "<$1>");
            press(&mut editor, "RET");
            editor
        })
        .bench_local_values(|mut editor| {
            press(&mut editor, "!");
            black_box(editor.window_buffer().text().len())
        });
}

/// The regexp's matches lit on a screen at the start of one long line, as each frame does.
#[divan::bench(args = [20_000_000], sample_count = 20)]
fn lit_regexp_matches_on_a_long_line(bencher: Bencher, bytes: usize) {
    let mut editor = one_long_line(bytes);
    press(&mut editor, "C-M-s");
    type_text(&mut editor, r"\d+");
    bencher.bench_local(|| black_box(editor.decorations(0..8_000).len()));
}

/// One key typed and taken back in a search for a string on every line, a few times each.
#[divan::bench(args = [100_000], sample_count = 20)]
fn key_in_search_for_common_string(bencher: Bencher, lines: usize) {
    let mut editor = searching(lines, "e");
    bencher.bench_local(|| {
        type_text(&mut editor, "x");
        press(&mut editor, "DEL");
        black_box(editor.minibuffer().unwrap().candidates().len())
    });
}
