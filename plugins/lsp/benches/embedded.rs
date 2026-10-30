//! `cargo bench -p remacs-lsp --bench embedded`: what keeping the document of a page's
//! scripts up to date costs per keystroke, on a page of about 600 KB.

use std::ops::Range;

use divan::{Bencher, black_box};
use remacs_lsp::embedded::Mirror;
use remacs_text::{Snapshot, Text};

#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

const SECTIONS: usize = 3000;

fn main() {
    divan::main();
}

/// The page and where its scripts are.
fn page() -> (Text, Vec<Range<usize>>) {
    let mut page = String::new();
    let mut scripts = Vec::new();
    for index in 0..SECTIONS {
        page.push_str(&format!(
            "<section id=\"s{index}\"><h2>Part {index}</h2>\n<p>Some text, <b>bold</b>.</p>\n"
        ));
        page.push_str("<script>\n");
        let start = page.len();
        page.push_str(&format!("const part{index} = document.getElementById(\"s{index}\");\n"));
        scripts.push(start..page.len());
        page.push_str("</script></section>\n");
    }
    (Text::from(page.as_str()), scripts)
}

/// `ranges` with the one at `index` grown by `by` bytes and the later ones moved.
fn grown(ranges: &[Range<usize>], index: usize, by: usize) -> Vec<Range<usize>> {
    let grow = |offset: usize, from: usize| if offset >= from { offset + by } else { offset };
    let from = ranges[index].end;
    ranges.iter().map(|range| grow(range.start, from + 1)..grow(range.end, from)).collect()
}

/// A character typed in the middle script: the edit is made on the mirror too.
#[divan::bench]
fn typing_in_a_script(bencher: Bencher) {
    let (mut text, scripts) = page();
    let middle = SECTIONS / 2;
    let mut mirror = Mirror::new("JavaScript", text.snapshot(), scripts.clone());
    let mut ranges = scripts;
    bencher.bench_local(|| {
        text.insert(ranges[middle].start + 1, "x");
        ranges = grown(&ranges, middle, 1);
        black_box(mirror.update(&text.snapshot(), ranges.clone()));
    });
}

/// The scripts' bounds as found again differ from the edited ones: all is blanked again.
#[divan::bench]
fn scripts_found_again(bencher: Bencher) {
    let (text, scripts) = page();
    let snapshot: Snapshot = text.snapshot();
    let moved: Vec<_> = scripts.iter().map(|range| range.start..range.end - 1).collect();
    let mut mirror = Mirror::new("JavaScript", snapshot.clone(), scripts.clone());
    let mut flip = false;
    bencher.bench_local(|| {
        flip = !flip;
        let ranges = if flip { moved.clone() } else { scripts.clone() };
        black_box(mirror.update(&snapshot, ranges));
    });
}
