//! A test binary of its own: dhat replaces the global allocator.

#[allow(dead_code)]
#[path = "../src/dap.rs"]
mod dap;

use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use dap::{Connection, Incoming};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// About 100 bytes a message: more than the profiled adapter sends in `BUSY`.
const PRINTED: usize = 8 * 1024 * 1024;
/// `run.toml`'s default `output-backlog`.
const BACKLOG: usize = 16;
const BUSY: Duration = Duration::from_secs(1);
/// Unbounded, the adapter queued 11 MB in `BUSY`.
const MAX_PEAK: usize = 512 * 1024;
/// An adapter telling of a program that prints a line after another.
const FLOOD: &str = r#"body='{"type":"event","event":"output","body":{"category":"stdout","output":"y\n"}}'
yes "$(printf 'Content-Length: %d\r\n\r\n%s' ${#body} "$body")" | head -c "$0""#;

#[test]
fn a_flooding_adapter_waits_for_a_busy_editor_instead_of_its_messages_piling_up() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let connection = Connection::stdio(
        Path::new("/bin/sh"),
        &["-c".into(), FLOOD.into(), PRINTED.to_string()],
        &std::env::temp_dir(),
        BACKLOG,
        Arc::new(|| {}),
    )
    .expect("starting the shell");
    thread::sleep(BUSY);
    let peak = dhat::HeapStats::get().max_bytes;
    let mut messages = 0;
    loop {
        match connection.try_receive() {
            Some(Incoming::Message(_)) => messages += 1,
            Some(Incoming::Log(_)) => {}
            Some(Incoming::Closed(_)) => break,
            None => thread::sleep(Duration::from_millis(1)),
        }
    }
    eprintln!("peak heap {peak} B while the editor was busy for {BUSY:?}");
    assert!(messages > PRINTED / 128, "{messages} messages");
    dhat::assert!(peak < MAX_PEAK);
}
