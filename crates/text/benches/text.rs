//! `cargo bench -p remacs-text --bench text`

use divan::{Bencher, black_box};
use remacs_text::Text;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

/// Snapshots a hook pass takes per key: syntax, LSP, diagnostics, git, completion…
const SNAPSHOTS_PER_KEY: usize = 12;

fn main() {
    divan::main();
}

fn typed(bytes: usize) -> Text {
    let mut text = Text::from("fn main() {}\n");
    for _ in 0..bytes {
        let end = text.len();
        text.insert(end, "x");
    }
    text
}

/// Taking a snapshot after `typed` bytes went into the open add-buffer chunk.
#[divan::bench(args = [0, 32 * 1024, 63 * 1024])]
fn snapshot_after_typing(bencher: Bencher, typed_bytes: usize) {
    let text = typed(typed_bytes);
    bencher.bench_local(|| black_box(text.snapshot()));
}

/// One key typed into 32 KiB of earlier typing, then the hooks' snapshots, one of them
/// kept until the next key as the parser's is.
#[divan::bench]
fn key_with_hook_snapshots(bencher: Bencher) {
    let mut text = typed(32 * 1024);
    let mut kept = text.snapshot();
    bencher.bench_local(|| {
        let end = text.len();
        text.insert(end, "y");
        for _ in 0..SNAPSHOTS_PER_KEY - 1 {
            black_box(text.snapshot());
        }
        kept = text.snapshot();
        black_box(&kept);
    });
}

fn lines(count: usize) -> String {
    (0..count).map(|line| format!("    let value_{line} = compute({line}, \"text\");\n")).collect()
}

/// Saving a text of `len` bytes, as C-x C-s does on the main thread.
#[divan::bench(args = [1024, 10 << 20], sample_count = 20)]
fn save(bencher: Bencher, len: usize) {
    let directory = std::env::temp_dir().join(format!("remacs-bench-save-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("saved-{len}.rs"));
    let mut text = Text::from(lines(len / 40).as_str());
    text.save_as(&path).unwrap();
    bencher.bench_local(|| text.save().unwrap());
    let _ = std::fs::remove_dir_all(directory);
}

/// Opening a UTF-8 file of `len` bytes: read into memory below 32 MiB, mapped above.
#[divan::bench(args = [1 << 20, 16 << 20, 64 << 20], sample_count = 20)]
fn open(bencher: Bencher, len: usize) {
    let path = std::env::temp_dir().join(format!("remacs-bench-open-{len}-{}", std::process::id()));
    std::fs::write(&path, lines(len / 40)).unwrap();
    bencher.bench_local(|| black_box(Text::open(&path).unwrap()));
    let _ = std::fs::remove_file(path);
}

#[divan::bench(args = [4 << 20])]
fn bytes_in_whole_text(bencher: Bencher, len: usize) {
    let source = lines(len / 40);
    let mut text = Text::from(source.as_str());
    // A few edits, so the text spans several pieces as an edited buffer does.
    for offset in [len / 4, len / 2, 3 * len / 4] {
        let offset = (offset..).find(|&o| source.is_char_boundary(o)).unwrap();
        text.insert(offset, "edit");
    }
    let snapshot = text.snapshot();
    bencher.bench_local(|| black_box(snapshot.bytes_in(0..snapshot.len())));
}
