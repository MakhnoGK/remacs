//! A test binary of its own: dhat replaces the global allocator.

#[allow(dead_code)]
#[path = "../src/prepare.rs"]
mod prepare;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use prepare::{Plan, Preparation, Prepared, Target};
use remacs_run::Event;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Two bytes a line: more lines than the profiled steps print in `BUSY`.
const PRINTED: usize = 256 * 1024;
/// `run.toml`'s default `output-backlog`.
const BACKLOG: usize = 16;
const BUSY: Duration = Duration::from_secs(1);
/// Unbounded, the steps queued 1.8 MB in `BUSY`.
const MAX_PEAK: usize = 512 * 1024;

#[test]
fn a_flooding_preparation_waits_for_a_busy_console_instead_of_its_output_piling_up() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let plan = Plan {
        before: vec![format!("yes | head -c {PRINTED}")],
        cargo_build: None,
        target: Target {
            program: "true".into(),
            args: Vec::new(),
            cwd: std::env::temp_dir(),
            env: Vec::new(),
        },
    };
    let preparation = Preparation::start(plan, BACKLOG, Arc::new(|| {}));
    thread::sleep(BUSY);
    let peak = dhat::HeapStats::get().max_bytes;
    let mut read = 0;
    let ready = loop {
        while let Some(event) = preparation.try_output() {
            if let Event::Output(bytes) = event {
                read += bytes.len();
            }
        }
        match preparation.try_result() {
            Some(prepared) => break matches!(prepared, Prepared::Ready(_)),
            None => thread::sleep(Duration::from_millis(1)),
        }
    };
    while let Some(Event::Output(bytes)) = preparation.try_output() {
        read += bytes.len();
    }
    eprintln!("peak heap {peak} B while the console was busy for {BUSY:?}");
    assert!(ready);
    assert_eq!(read, PRINTED);
    dhat::assert!(peak < MAX_PEAK);
}
