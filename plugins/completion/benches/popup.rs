//! `cargo bench -p remacs-completion --bench popup`

use divan::{Bencher, black_box};
use remacs_completion::{CompletionPlugin, Item, List, deliver, take_requests};
use remacs_core::{Buffer, Editor, Key, KeyCode, Modifiers, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_text::Text;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const WORDS: &[&str] = &[
    "iter", "map", "filter", "collect", "push", "extend", "insert", "remove", "contains",
    "sort", "split", "with", "capacity", "clone", "into", "as", "ref", "mut", "len", "get",
];

fn main() {
    divan::main();
}

fn type_char(editor: &mut Editor, character: char) {
    editor.handle_key(Key::new(Modifiers::default(), KeyCode::Char(character)));
}

/// A popup open on `items` items, as rust-analyzer lists a type's methods and every
/// importable name, after `s` was typed.
fn popup(items: usize) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &CompletionPlugin]);
    editor.add_buffer(Buffer::new("lib.rs", Text::from("value."), "Rust"));
    editor.window_buffer_mut().set_point(6);
    type_char(&mut editor, 's');
    take_requests(&mut editor);
    let items = (0..items)
        .map(|index| {
            let label = format!(
                "{}_{}_{index}",
                WORDS[index % WORDS.len()],
                WORDS[index / WORDS.len() % WORDS.len()]
            );
            Item {
                filter_text: label.clone(),
                insert: label.clone(),
                label,
                start: 6,
                ..Item::default()
            }
        })
        .collect();
    deliver(
        &mut editor,
        1,
        List {
            items,
            incomplete: false,
        },
    );
    assert!(!editor.popups().is_empty());
    editor
}

/// One more letter of the word typed and erased again: the list filtered twice.
#[divan::bench(args = [5_000], sample_count = 50)]
fn key_in_popup(bencher: Bencher, items: usize) {
    let mut editor = popup(items);
    let erase = parse_sequence("DEL").unwrap();
    bencher.bench_local(|| {
        type_char(&mut editor, 'e');
        for key in &erase {
            editor.handle_key(*key);
        }
        black_box(editor.popups().len())
    });
}
