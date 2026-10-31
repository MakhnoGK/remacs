//! `cargo bench -p remacs-git --bench blame`

use divan::{Bencher, black_box};
use remacs_git::blame::parse_blame;

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

fn main() {
    divan::main();
}

/// `git blame --porcelain` of a file of `lines` lines last changed by `commits` commits,
/// each commit owning runs of 10 lines in turn.
fn porcelain(lines: usize, commits: usize) -> Vec<u8> {
    let mut printed = String::new();
    let mut described = vec![false; commits];
    for line in 0..lines {
        let commit = (line / 10) % commits;
        let id = format!("{commit:040x}");
        printed += &format!("{id} {} {}\n", line + 1, line + 1);
        if !described[commit] {
            described[commit] = true;
            printed += &format!("author Author {commit}\nauthor-time {commit}\nfilename lib.rs\n");
        }
        printed += &format!("\tlet line_{line} = {line};\n");
    }
    printed.into_bytes()
}

#[divan::bench(args = [(10_000, 100), (100_000, 5_000)])]
fn parse(bencher: Bencher, (lines, commits): (usize, usize)) {
    let printed = porcelain(lines, commits);
    bencher.bench(|| parse_blame(black_box(&printed)));
}
