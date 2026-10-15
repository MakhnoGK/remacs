//! `cargo bench -p remacs-lsp --bench position`

use divan::{Bencher, black_box};
use lsp_types::Position;
use remacs_lsp::position::{Encoding, from_lsp};
use remacs_text::Text;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const DIAGNOSTICS: u32 = 1_000;

fn main() {
    divan::main();
}

/// A published diagnostic's start and end, as a UTF-16 server sends them, on a minified
/// line of `line_len` bytes.
#[divan::bench(args = [1 << 20])]
fn diagnostics_on_a_long_line(bencher: Bencher, line_len: usize) {
    let line = "const é = [1, 2];".repeat(line_len / 18);
    let text = Text::from(format!("// header\n{line}\n").as_str()).snapshot();
    let step = (line.encode_utf16().count() as u32) / DIAGNOSTICS;
    bencher.bench_local(|| {
        for index in 0..DIAGNOSTICS {
            let start = Position::new(1, index * step);
            let end = Position::new(1, index * step + 5);
            black_box(from_lsp(&text, start, Encoding::Utf16));
            black_box(from_lsp(&text, end, Encoding::Utf16));
        }
    });
}
