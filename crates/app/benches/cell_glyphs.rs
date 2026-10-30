//! `cargo bench -p remacs-app --bench cell_glyphs`

// The app is a binary, so the module is compiled in here on its own; it needs only gpui.
// Its test helpers are dead when the bench is built as a test.
#[path = "../src/cell_glyphs.rs"]
#[allow(dead_code)]
mod cell_glyphs;

use divan::{Bencher, black_box};
use gpui::px;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const COLUMNS: usize = 155;
const GRAPH_ROWS: usize = 10;
const TEXT_ROWS: usize = 16;

fn main() {
    divan::main();
}

/// A screen like btop's: framed panels, braille graphs, and rows of text between borders.
fn screen() -> Vec<String> {
    let inner = COLUMNS - 2;
    let border = |left: char, right: char| format!("{left}{}{right}", "─".repeat(inner));
    let graph: String = "⣿⣷⣦⡀⢀⣤⣶⠀".chars().cycle().take(inner).collect();
    let text =
        format!("{:inner$}", "  2714 iTerm2   /Applications/iTerm.app/Contents/MacOS/iTerm2");
    let mut rows = vec![border('┌', '┐')];
    rows.extend((0..GRAPH_ROWS).map(|_| format!("│{graph}│")));
    rows.push(border('├', '┤'));
    rows.extend((0..TEXT_ROWS).map(|_| format!("│{text}│")));
    rows.push(border('└', '┘'));
    rows
}

/// What a frame of that screen takes to turn its graphics into pieces.
#[divan::bench(sample_count = 50)]
fn frame_of_graphics(bencher: Bencher) {
    let rows = screen();
    bencher.bench_local(|| {
        for row in &rows {
            black_box(cell_glyphs::split(row, COLUMNS, px(8.4), px(23.0)));
        }
    });
}
