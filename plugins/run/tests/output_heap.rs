//! A test binary of its own: dhat replaces the global allocator.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use remacs_run::{Event, Launch, Process, Program};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const PRINTED: usize = 512 * 1024 * 1024;
/// `run.toml`'s default `output-backlog`.
const BACKLOG: usize = 16;
/// Long enough for `yes` to print it all, about 4 GB/s through a pipe.
const BUSY: Duration = Duration::from_secs(1);
const MAX_PEAK: usize = 8 * 1024 * 1024;

#[test]
fn a_fast_program_waits_for_a_busy_console_instead_of_its_output_piling_up() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let process = Process::start(
        Launch {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), format!("yes | head -c {PRINTED}")],
            cwd: std::env::temp_dir(),
            env: Vec::new(),
            before: Vec::new(),
            shown: None,
        },
        BACKLOG,
        Arc::new(|| {}),
    );
    thread::sleep(BUSY);
    let peak = dhat::HeapStats::get().max_bytes;
    let mut read = 0;
    loop {
        match process.try_event() {
            Some(Event::Output(bytes)) => read += bytes.len(),
            Some(Event::Exited(_)) => break,
            Some(Event::Started(_)) => {}
            None => thread::sleep(Duration::from_millis(1)),
        }
    }
    eprintln!("peak heap {peak} B while the console was busy for {BUSY:?}");
    assert_eq!(read, PRINTED);
    dhat::assert!(peak < MAX_PEAK);
}
