use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use notify::{Event, EventKind, RecursiveMode, Watcher};
use remacs_core::Notifier;

use crate::entry::{Entry, read_directory};

/// File system events arriving this close together cause one read.
const SETTLE_TIME: Duration = Duration::from_millis(50);

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
        loop {
            match messages.recv_timeout(SETTLE_TIME) {
                Ok(Message::Changed) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}
