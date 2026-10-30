//! `cargo run --release -p remacs-text --example open -- <kind> [size_mib]`
//!
//! Opens a generated file of `kind` (utf8, binary, latin1, cp1251, utf16) and reports the
//! time, the peak heap and the peak resident memory the open took.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use remacs_text::Text;

struct PeakHeap;

static HEAP: AtomicUsize = AtomicUsize::new(0);
static PEAK_HEAP: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator, only counting bytes.
unsafe impl GlobalAlloc for PeakHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now = HEAP.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        PEAK_HEAP.fetch_max(now, Ordering::Relaxed);
        // SAFETY: the caller's layout, passed on.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        HEAP.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: allocated by `alloc` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: PeakHeap = PeakHeap;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let kind = args.next().unwrap_or_else(|| "latin1".into());
    let size_mib: usize = args.next().map_or(1024, |size| size.parse().expect("size in MiB"));
    let path = format!("target/open-{kind}-{size_mib}m.txt");
    let path = Path::new(&path);
    if !path.exists() {
        generate(path, &kind, size_mib << 20)?;
    }

    let heap_before = HEAP.load(Ordering::Relaxed);
    PEAK_HEAP.store(heap_before, Ordering::Relaxed);
    let started = Instant::now();
    let text = Text::open(path)?;
    let elapsed = started.elapsed();
    let middle = text.line_start(text.line_count() / 2).unwrap_or(0);
    println!(
        "{kind:<7} {:>9.1} ms  peak heap {:>8.1} MiB  peak RSS {:>8.1} MiB  {} lines, line {} at {middle}, {:?}",
        elapsed.as_secs_f64() * 1e3,
        (PEAK_HEAP.load(Ordering::Relaxed) - heap_before) as f64 / (1 << 20) as f64,
        peak_rss_mib(),
        text.line_count(),
        text.line_count() / 2,
        text.format(),
    );
    Ok(())
}

fn generate(path: &Path, kind: &str, target_len: usize) -> std::io::Result<()> {
    let mut writer = BufWriter::with_capacity(1 << 20, File::create(path)?);
    let mut written = 0;
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    if kind == "utf16" {
        writer.write_all(&[0xFF, 0xFE])?;
    }
    for line in 0.. {
        if written >= target_len {
            break;
        }
        let bytes: Vec<u8> = match kind {
            "utf8" => format!("line {line}: café crème, привет\n").into_bytes(),
            "latin1" => format!("ligne {line}: café crème brûlée\n")
                .chars()
                .map(|character| character as u8)
                .collect(),
            "cp1251" => format!("строка {line}: привет, мир\n")
                .chars()
                .map(|character| match character {
                    'А'..='я' => (character as u32 - 'А' as u32 + 0xC0) as u8,
                    _ => character as u8,
                })
                .collect(),
            "utf16" => {
                format!("line {line}: café €\n").encode_utf16().flat_map(u16::to_le_bytes).collect()
            }
            "binary" => (0..4096)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    (seed >> 24) as u8 & if line == 0 { 0 } else { 0xFF }
                })
                .collect(),
            _ => panic!("kind is one of utf8, binary, latin1, cp1251, utf16"),
        };
        writer.write_all(&bytes)?;
        written += bytes.len();
    }
    writer.flush()
}

fn peak_rss_mib() -> f64 {
    // SAFETY: getrusage only writes into the zeroed struct we pass.
    let max_rss_bytes = unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        usage.ru_maxrss
    };
    max_rss_bytes as f64 / (1 << 20) as f64
}
