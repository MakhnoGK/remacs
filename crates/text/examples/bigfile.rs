//! `cargo run --release -p remacs-text --example bigfile -- [size_mib] [path]`

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use remacs_text::Text;

const VIEWPORT_LINES: usize = 50;
const RANDOM_EDITS: usize = 10_000;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let size_mib: usize = args
        .next()
        .map_or(2048, |size| size.parse().expect("size in MiB"));
    let path = args.next().map_or_else(
        || PathBuf::from(format!("target/bigfile-{size_mib}m.txt")),
        PathBuf::from,
    );

    if !path.exists() {
        measure("generate", || generate(&path, size_mib << 20))?;
    }

    let mut text = measure("open", || Text::open(&path))?;
    println!(
        "  {} bytes, {} lines, peak RSS {}",
        text.len(),
        text.line_count(),
        peak_rss_mib()
    );

    let middle_line = text.line_count() / 2;
    let middle = measure("seek to middle line", || {
        text.line_start(middle_line).unwrap()
    });
    measure("read viewport", || {
        read_lines(&text, middle_line, VIEWPORT_LINES)
    });

    let mut seed = middle as u64 | 1;
    measure("random edits", || {
        for edit in 0..RANDOM_EDITS {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let offset = (seed % text.len() as u64) as usize;
            if edit % 4 == 0 {
                text.delete(offset..(offset + 8).min(text.len()));
            } else {
                text.insert(offset, "edit\n");
            }
        }
    });
    measure("seek after edits", || text.line_start(middle_line).unwrap());
    measure("read viewport after edits", || {
        read_lines(&text, middle_line, VIEWPORT_LINES)
    });
    measure("offset to line", || text.line_of(text.len() / 3));
    println!("peak RSS {}", peak_rss_mib());
    Ok(())
}

fn generate(path: &Path, target_len: usize) -> std::io::Result<()> {
    let mut writer = BufWriter::with_capacity(1 << 20, File::create(path)?);
    let mut written = 0;
    for line in 0.. {
        if written >= target_len {
            break;
        }
        let indent = "    ".repeat(line % 4);
        let line = format!(
            "{indent}let value_{line} = compute(&input[{line}..], {});\n",
            line % 97
        );
        writer.write_all(line.as_bytes())?;
        written += line.len();
    }
    writer.flush()
}

fn read_lines(text: &Text, first_line: usize, count: usize) -> usize {
    (first_line..first_line + count)
        .filter_map(|line| text.line_range(line))
        .map(|range| text.bytes_in(range).len())
        .sum()
}

fn measure<T>(label: &str, operation: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let result = operation();
    println!(
        "{label:<28} {:>10.3} ms",
        started.elapsed().as_secs_f64() * 1e3
    );
    result
}

fn peak_rss_mib() -> String {
    // SAFETY: getrusage only writes into the zeroed struct we pass.
    let max_rss_bytes = unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        usage.ru_maxrss
    };
    format!("{:.1} MiB", max_rss_bytes as f64 / (1 << 20) as f64)
}
