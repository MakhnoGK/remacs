//! `cargo bench -p remacs-editing --bench cursors`
//!
//! Adding a cursor at every match of the region reads the whole buffer, which may be
//! megabytes with thousands of matches; then every edit runs at each of them.

use divan::Bencher;
use remacs_core::{Buffer, Editor};
use remacs_editing::{EditingPlugin, EditingSettings};
use remacs_text::Text;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const MATCHES: usize = 10_000;
const LINE_LEN: usize = 800;

fn main() {
    divan::main();
}

/// An editor over `text` with the region on its first two bytes and no limit on cursors.
fn editor_over(text: &str) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin]);
    editor.plugin_state::<EditingSettings>().max_cursors = usize::MAX;
    editor.add_buffer(Buffer::new("text", Text::from(text), "Fundamental"));
    select_first_two_bytes(&mut editor);
    editor
}

fn select_first_two_bytes(editor: &mut Editor) {
    let buffer = editor.buffer_mut();
    buffer.clear_cursors();
    buffer.set_mark(0);
    buffer.set_point(2);
    buffer.activate_mark();
}

#[divan::bench]
fn mark_all_like_this(bencher: Bencher) {
    let line = format!("ab{}\n", "x".repeat(LINE_LEN - 3));
    let mut editor = editor_over(&line.repeat(MATCHES));
    bencher.bench_local(|| {
        select_first_two_bytes(&mut editor);
        editor.run_command("mc/mark-all-like-this");
        assert_eq!(editor.buffer().cursors().len(), MATCHES - 1);
    });
}

/// Kills right after kills grow one kill ring entry, as holding C-k does.
#[divan::bench]
fn consecutive_kills(bencher: Bencher) {
    const LINES: usize = 5_000;
    let text = format!("{}\n", "x".repeat(79)).repeat(LINES);
    bencher
        .with_inputs(|| {
            let mut editor = editor_over(&text);
            editor.buffer_mut().set_point(0);
            editor
        })
        .bench_local_values(|mut editor| {
            for _ in 0..2 * LINES {
                editor.run_command("kill-line");
            }
            assert_eq!(editor.kill_ring.newest().map(str::len), Some(text.len()));
            editor
        });
}

/// An edit runs at each cursor, and each moves every cursor after it; transposing keeps
/// the text as long, so every run does the same work.
#[divan::bench(args = [100, 1_000, 10_000])]
fn edit_at_cursors(bencher: Bencher, cursors: usize) {
    let mut editor = editor_over(&"ab\n".repeat(cursors));
    editor.run_command("mc/mark-all-like-this");
    editor.run_command("keyboard-quit");
    assert_eq!(editor.buffer().cursors().len(), cursors - 1);
    bencher.bench_local(|| editor.run_command("transpose-chars"));
}
