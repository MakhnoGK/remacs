//! `cargo bench -p remacs-lsp --bench semantic_tokens`

use divan::{Bencher, black_box};
use remacs_lsp::position::Encoding;
use remacs_lsp::semantic::decode;
use remacs_text::Text;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const LINE: &str = "    let value = compute(first, second) + other.field;\n";
/// Five tokens of `LINE`: start, length, as LSP columns.
const TOKENS: [(u32, u32); 5] = [(4, 3), (8, 5), (16, 7), (24, 5), (31, 6)];

fn main() {
    divan::main();
}

/// The tokens of `lines` copies of `LINE`, relative to the one before.
fn data(lines: u32, one_line: bool) -> Vec<u32> {
    let mut data = Vec::new();
    let mut previous: Option<u32> = None;
    for copy in 0..lines {
        let shift = if one_line { copy * LINE.trim_end().len() as u32 } else { 0 };
        for (index, (start, length)) in TOKENS.into_iter().enumerate() {
            let start = shift + start;
            let new_line = !one_line && index == 0 && copy > 0;
            let delta = match previous {
                Some(previous) if !new_line => start - previous,
                _ => start,
            };
            data.extend([u32::from(new_line), delta, length, 0, 0]);
            previous = Some(start);
        }
    }
    data
}

/// A file of a megabyte, 20 000 lines of five tokens each.
#[divan::bench]
fn a_megabyte_of_lines(bencher: Bencher) {
    let text = Text::from(LINE.repeat(20_000).as_str()).snapshot();
    let data = data(20_000, false);
    bencher.bench(|| decode(black_box(&data), &text, Encoding::Utf16));
}

/// Minified: the same tokens on one line of a megabyte.
#[divan::bench]
fn a_megabyte_on_one_line(bencher: Bencher) {
    let text = Text::from(LINE.trim_end().repeat(20_000).as_str()).snapshot();
    let data = data(20_000, true);
    bencher.bench(|| decode(black_box(&data), &text, Encoding::Utf16));
}
