//! Finds entry points on a thread of its own: a big Rust file takes a full parse and
//! reads of `Cargo.toml` files, too slow for the moment typing pauses.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use remacs_core::Notifier;
use remacs_text::Snapshot;

use crate::entry_points::{EntryPoint, entry_points};

/// Larger files are not looked through for entry points.
const MAX_SCANNED_BYTES: usize = 4 << 20;

/// Where the scanner leaves a buffer's newest entry points.
pub type ScanSlot = Arc<Mutex<Option<Scan>>>;

pub struct Request {
    pub path: PathBuf,
    pub mode: &'static str,
    pub text: Snapshot,
    pub root: PathBuf,
    pub slot: ScanSlot,
}

/// The entry points of a version of a buffer's text.
pub struct Scan {
    pub text: Snapshot,
    pub root: PathBuf,
    pub points: Vec<EntryPoint>,
}

/// One thread scans for every buffer; it starts with the first request.
#[derive(Default)]
pub struct Scanner(Option<Sender<Request>>);

impl Scanner {
    pub fn send(&mut self, request: Request, notifier: Notifier) {
        let requests = self.0.get_or_insert_with(|| {
            let (requests, received) = mpsc::channel();
            thread::Builder::new()
                .name("entry-points".into())
                .spawn(move || run(received, notifier))
                .expect("spawning the entry point thread");
            requests
        });
        // The thread only stops when this sender is dropped.
        let _ = requests.send(request);
    }
}

fn run(requests: Receiver<Request>, notifier: Notifier) {
    while let Ok(first) = requests.recv() {
        for request in newest_per_buffer(first, &requests) {
            // The buffer was killed.
            if Arc::strong_count(&request.slot) == 1 {
                continue;
            }
            let points = if request.text.len() > MAX_SCANNED_BYTES {
                Vec::new()
            } else {
                let bytes = request.text.bytes_in(0..request.text.len());
                let contents = String::from_utf8_lossy(&bytes);
                entry_points(&request.path, request.mode, &contents, &request.root)
            };
            *request.slot.lock().expect("scan slots are never poisoned") = Some(Scan {
                text: request.text,
                root: request.root,
                points,
            });
            notifier();
        }
    }
}

/// Requests queued while the previous scan ran; older ones for the same buffer are stale.
fn newest_per_buffer(first: Request, requests: &Receiver<Request>) -> Vec<Request> {
    let mut newest: Vec<Request> = Vec::new();
    for request in std::iter::once(first).chain(requests.try_iter()) {
        match newest
            .iter_mut()
            .find(|queued| Arc::ptr_eq(&queued.slot, &request.slot))
        {
            Some(queued) => *queued = request,
            None => newest.push(request),
        }
    }
    newest
}
