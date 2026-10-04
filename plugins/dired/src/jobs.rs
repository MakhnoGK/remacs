use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use remacs_core::Editor;

use crate::listing::Listing;

/// File operations running in the background, reported once done.
#[derive(Default)]
pub struct Jobs {
    reports: Vec<Receiver<Report>>,
}

/// How a file operation went.
pub struct Report {
    /// Listings of these are read again.
    pub directories: Vec<PathBuf>,
    /// Files now elsewhere, from and to; buffers visiting them follow.
    pub moved: Vec<(PathBuf, PathBuf)>,
    pub message: String,
}

/// Runs `job` on a background thread, saying `busy` meanwhile; [`pick_up`] reports it.
pub fn run(editor: &mut Editor, busy: &'static str, job: impl FnOnce() -> Report + Send + 'static) {
    let (send, receive) = mpsc::channel();
    let notifier = editor.notifier();
    thread::Builder::new()
        .name("dired-job".into())
        .spawn(move || {
            let _ = send.send(job());
            notifier();
        })
        .expect("spawning a dired job thread");
    editor.plugin_state::<Jobs>().reports.push(receive);
    editor.set_message(busy);
}

/// Says how finished jobs went, points buffers at moved files and reads the listings
/// they changed again, without waiting for FSEvents.
pub fn pick_up(editor: &mut Editor) {
    let mut reports = Vec::new();
    editor
        .plugin_state::<Jobs>()
        .reports
        .retain(|receiver| match receiver.try_recv() {
            Ok(report) => {
                reports.push(report);
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
    for report in reports {
        for (from, to) in &report.moved {
            editor.file_moved(from, to);
        }
        for buffer in editor.buffers() {
            let changed = buffer
                .default_directory()
                .is_some_and(|directory| report.directories.iter().any(|each| each == directory));
            if changed && let Some(listing) = buffer.local::<Listing>() {
                listing.reader.reread();
            }
        }
        editor.set_message(report.message);
    }
}

/// `Could not move a: error`, with how many more failed.
pub fn failure_message(verb: &str, failures: &[String]) -> Option<String> {
    let (first, rest) = failures.split_first()?;
    Some(if rest.is_empty() {
        format!("Could not {verb} {first}")
    } else {
        format!("Could not {verb} {first} (and {} more)", rest.len())
    })
}
