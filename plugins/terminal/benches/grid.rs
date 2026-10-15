//! `cargo bench -p remacs-terminal --bench grid`

use std::ops::RangeInclusive;

use alacritty_terminal::Term;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Config;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi::Processor;
use divan::{Bencher, black_box};
use remacs_terminal::grid::{Overlay, rows};

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const COLUMNS: usize = 200;
const SCREEN_LINES: usize = 50;
const PRINTED_LINES: usize = 10_000;
/// Matches on each line, as a search for a common letter finds.
const MATCHES_PER_LINE: usize = 5;

fn main() {
    divan::main();
}

/// A terminal that printed `PRINTED_LINES` lines, and `matches` search matches spread
/// over its scrollback, sorted as the search finds them.
fn searched(matches: usize) -> (Term<VoidListener>, Vec<RangeInclusive<Point>>) {
    let size = TermSize::new(COLUMNS, SCREEN_LINES);
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let mut parser: Processor = Processor::new();
    for line in 0..PRINTED_LINES {
        parser.advance(&mut term, format!("line {line}: some output text\r\n").as_bytes());
    }
    let top = SCREEN_LINES as i32 - PRINTED_LINES as i32;
    let found = (0..matches)
        .map(|index| {
            let line = Line(top + (index / MATCHES_PER_LINE) as i32);
            let column = Column(10 + 3 * (index % MATCHES_PER_LINE));
            Point::new(line, column)..=Point::new(line, column + 1)
        })
        .collect();
    (term, found)
}

/// A frame of the terminal while a search shows its matches.
#[divan::bench(args = [0, 1_000, 50_000], sample_count = 30)]
fn frame_while_searching(bencher: Bencher, matches: usize) {
    let (term, found) = searched(matches);
    let commands = Default::default();
    let overlay = Overlay {
        focused: true,
        matches: &found,
        current_match: found.last(),
        commands: &commands,
        link: None,
    };
    bencher.bench_local(|| black_box(rows(&term, &overlay)));
}
