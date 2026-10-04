use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::mem;
use std::ops::Range;
use std::path::PathBuf;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use remacs_core::Notifier;
use remacs_format::{Failure, Formatted, Request};
use remacs_text::Snapshot;
use serde_json::{Value, json};

const SCRIPT: &str = include_str!("worker.mjs");

struct Job {
    id: u64,
    path: PathBuf,
    text: Snapshot,
    range: Option<Range<usize>>,
}

/// The texts sent, by request, for diffing what comes back, and the answers.
#[derive(Default)]
struct Shared {
    sent: HashMap<u64, String>,
    formatted: Vec<(u64, Formatted)>,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared
        .lock()
        .expect("no thread panics holding the shared state")
}

/// The Node process that formats with prettier: one for all projects, each with its own
/// prettier, kept loaded so a format costs milliseconds rather than Node's start-up. A
/// writer thread sends the texts, a reader thread turns what comes back into edits.
pub(crate) struct Worker {
    jobs: Sender<Job>,
    shared: Arc<Mutex<Shared>>,
}

impl Worker {
    pub fn start(notifier: Notifier) -> io::Result<Self> {
        let mut child = Command::new("node")
            .args(["--input-type=module", "-e", SCRIPT])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let (jobs, queued) = mpsc::channel();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let writer_shared = Arc::clone(&shared);
        let writer_notifier = Arc::clone(&notifier);
        thread::Builder::new()
            .name("prettier writer".into())
            .spawn(move || {
                write_all(
                    BufWriter::new(stdin),
                    queued,
                    &writer_shared,
                    &writer_notifier,
                )
            })?;
        let reader_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("prettier reader".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if let Ok(answer) = serde_json::from_str(&line) {
                        answered(&reader_shared, answer);
                        notifier();
                    }
                }
                let mut shared = lock(&reader_shared);
                for id in mem::take(&mut shared.sent).into_keys() {
                    let stopped = Failure::Failed("prettier stopped".into());
                    shared.formatted.push((id, Err(stopped)));
                }
                drop(shared);
                notifier();
                let _ = child.wait();
            })?;
        Ok(Self { jobs, shared })
    }

    /// `Err` when the process has stopped, so a new one is needed.
    pub fn send(&self, request: Request) -> Result<(), ()> {
        let job = Job {
            id: request.id,
            path: request.path,
            text: request.text,
            range: request.range,
        };
        self.jobs.send(job).map_err(drop)
    }

    pub fn take_formatted(&self) -> Vec<(u64, Formatted)> {
        mem::take(&mut lock(&self.shared).formatted)
    }
}

/// Copies each text out of its snapshot off the UI thread, which may take a while for a
/// large file, and sends it with its region in UTF-16, as JavaScript counts.
fn write_all(
    mut stdin: BufWriter<ChildStdin>,
    queued: Receiver<Job>,
    shared: &Mutex<Shared>,
    notifier: &Notifier,
) {
    for job in queued {
        let Ok(text) = String::from_utf8(job.text.bytes_in(0..job.text.len())) else {
            let failure = Failure::Unsupported("Not formatted: the text is not UTF-8".into());
            lock(shared).formatted.push((job.id, Err(failure)));
            notifier();
            continue;
        };
        let utf16 = |offset: usize| text.get(..offset).unwrap_or(&text).encode_utf16().count();
        let range = job
            .range
            .map(|range| [utf16(range.start), utf16(range.end)]);
        let line = json!({ "id": job.id, "path": job.path, "text": text, "range": range });
        lock(shared).sent.insert(job.id, text);
        if writeln!(stdin, "{line}")
            .and_then(|()| stdin.flush())
            .is_err()
        {
            break;
        }
    }
}

fn answered(shared: &Mutex<Shared>, answer: Value) {
    let Some(id) = answer["id"].as_u64() else {
        return;
    };
    let Some(sent) = lock(shared).sent.remove(&id) else {
        return;
    };
    let formatted = if let Some(error) = answer["error"].as_str() {
        Err(Failure::Failed(format!("prettier: {error}")))
    } else if answer["ignored"] == true {
        let ignored = "Not formatted: .prettierignore lists the file";
        Err(Failure::Unsupported(ignored.into()))
    } else if let Some(text) = answer["text"].as_str() {
        Ok(remacs_format::edits(&sent, text))
    } else {
        Err(Failure::Failed("prettier answered nothing".into()))
    };
    lock(shared).formatted.push((id, formatted));
}
