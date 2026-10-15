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
