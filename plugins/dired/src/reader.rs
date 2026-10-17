use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecursiveMode, Watcher};
use remacs_core::Notifier;

use crate::entry::{Entry, read_directory};

/// File system events arriving this close together cause one read.
const SETTLE_TIME: Duration = Duration::from_millis(50);
/// A file changing without pause, like a growing log, is read at least this often.
const MAX_DELAY: Duration = Duration::from_millis(500);

/// Reads a directory on a background thread, and again whenever it changes.
pub struct DirectoryReader {
    messages: Sender<Message>,
    listings: Receiver<io::Result<Vec<Entry>>>,
}

enum Message {
    Changed,
    Stop,
}

impl DirectoryReader {
    pub fn start(directory: PathBuf, notifier: Notifier) -> Self {
        let (messages, received) = mpsc::channel();
        let (send_listing, listings) = mpsc::channel();
        let events = messages.clone();
        thread::Builder::new()
            .name("dired".into())
            .spawn(move || run(&directory, events, &received, &send_listing, &notifier))
            .expect("spawning the dired thread");
        Self { messages, listings }
    }

    pub fn reread(&self) {
        let _ = self.messages.send(Message::Changed);
    }

    /// The newest listing read since the last call, if any.
    pub fn latest(&self) -> Option<io::Result<Vec<Entry>>> {
        self.listings.try_iter().last()
    }
}

impl Drop for DirectoryReader {
    fn drop(&mut self) {
        let _ = self.messages.send(Message::Stop);
    }
}

fn run(
    directory: &Path,
    events: Sender<Message>,
    messages: &Receiver<Message>,
    listings: &Sender<io::Result<Vec<Entry>>>,
    notifier: &Notifier,
) {
    // Watching before the first read keeps changes made during it.
    let _watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        if !matches!(
            event,
            Ok(Event {
                kind: EventKind::Access(_),
                ..
            })
        ) {
            let _ = events.send(Message::Changed);
        }
    })
    .and_then(|mut watcher| {
        watcher.watch(directory, RecursiveMode::NonRecursive)?;
        Ok(watcher)
    });
    loop {
        if listings.send(read_directory(directory)).is_err() {
            return;
        }
        notifier();
        if !matches!(messages.recv(), Ok(Message::Changed)) {
            return;
        }
        let deadline = Instant::now() + MAX_DELAY;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match messages.recv_timeout(SETTLE_TIME.min(left)) {
                Ok(Message::Changed) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[test]
    fn a_file_growing_without_pause_is_listed_again_within_a_second() {
        let directory =
            std::env::temp_dir().join(format!("remacs-dired-growing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let log = directory.join("log.txt");
        fs::write(&log, "").unwrap();
        let (notify, notified) = mpsc::channel();
        let reader = DirectoryReader::start(
            directory.clone(),
            Arc::new(move || {
                let _ = notify.send(());
            }),
        );
        let log_size = |listing: io::Result<Vec<Entry>>| {
            let entries = listing.unwrap();
            entries
                .iter()
                .find(|entry| entry.name == "log.txt")
                .unwrap()
                .size
        };
        notified.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(log_size(reader.latest().unwrap()), 0);

        let stop = Arc::new(AtomicBool::new(false));
        let writing = Arc::clone(&stop);
        // Like `echo line >> log.txt` in a loop: FSEvents tells of writes as files close.
        let writer = thread::spawn(move || {
            while !writing.load(Ordering::Relaxed) {
                OpenOptions::new()
                    .append(true)
                    .open(&log)
                    .unwrap()
                    .write_all(b"line\n")
                    .unwrap();
                thread::sleep(Duration::from_millis(20));
            }
        });
        let started = Instant::now();
        let grown = loop {
            let left = Duration::from_secs(1).saturating_sub(started.elapsed());
            if notified.recv_timeout(left).is_err() {
                break false;
            }
            if reader.latest().is_some_and(|listing| log_size(listing) > 0) {
                break true;
            }
        };
        stop.store(true, Ordering::Relaxed);
        writer.join().unwrap();
        let _ = fs::remove_dir_all(&directory);
        assert!(grown, "no new listing within a second");
    }
}
