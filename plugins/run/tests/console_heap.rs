//! A test binary of its own: dhat replaces the global allocator.

use std::path::Path;

use remacs_run::console;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

// shortcut: 100k segments, not 1M — dhat takes ~2 min in release on 1M; the leak grows
// linearly, so 100k already shows it.
const SEGMENTS: usize = 100_000;
/// Past the first rebuild, which waits for 64 KB of appended text.
const WARM_UP: usize = SEGMENTS / 3;
const SEGMENTS_PER_READ: usize = 1_000;

fn feed(buffer: &mut remacs_core::Buffer, segments: std::ops::Range<usize>) {
    let mut read = String::new();
    for segment in segments {
        read.push_str(&format!("\r{}%", segment % 100));
        if read.len() >= SEGMENTS_PER_READ * 3 {
            console::append_output(buffer, read.as_bytes(), 10_000);
            read.clear();
        }
    }
    console::append_output(buffer, read.as_bytes(), 10_000);
}

#[test]
fn progress_redrawn_with_carriage_returns_keeps_the_peak_heap_flat() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let mut buffer = console::new_buffer("*run*".into(), 1, Path::new("/"), Path::new("/"));
    feed(&mut buffer, 0..WARM_UP);
    let warm_peak = dhat::HeapStats::get().max_bytes;
    feed(&mut buffer, WARM_UP..SEGMENTS);
    let peak = dhat::HeapStats::get().max_bytes;
    eprintln!("peak heap: {warm_peak} B after {WARM_UP} segments, {peak} B after {SEGMENTS}");
    assert_eq!(buffer.contents(0..buffer.text().len()), "99%");
    dhat::assert!(peak <= warm_peak + warm_peak / 10);
}
