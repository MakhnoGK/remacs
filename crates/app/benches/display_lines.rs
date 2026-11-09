//! `cargo bench -p remacs-app --bench display_lines`

// The app is a binary, so the module is compiled in here on its own; built as a test
// without a harness, its tests are dropped and leave their imports unused.
#[path = "../src/display_line.rs"]
#[allow(dead_code, unused_imports)]
mod display_line;

use display_line::DisplayLine;
use divan::{Bencher, black_box};

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

/// A tall window's rows.
const ROWS: usize = 60;
const LINE: &str =
    "        let parsed = configuration.sections().filter(|section| section.enabled).count();";

fn main() {
    divan::main();
}

/// A frame of lines as drawn without hints.
#[divan::bench(sample_count = 200)]
fn frame_without_hints(bencher: Bencher) {
    bencher.bench_local(|| {
        for _ in 0..ROWS {
            black_box(DisplayLine::new(black_box(LINE)));
        }
    });
}

/// The same with three hints on every line, as rust-analyzer gives a chain: a binding's
/// type, a parameter name and the chain's type at the end.
#[divan::bench(sample_count = 200)]
fn frame_with_hints(bencher: Bencher) {
    let hints = [(14, ": usize"), (48, "f: "), (LINE.len(), " usize")];
    bencher.bench_local(|| {
        for _ in 0..ROWS {
            let mut line = DisplayLine::new(black_box(LINE));
            line.insert_hints(hints);
            black_box(line);
        }
    });
}
