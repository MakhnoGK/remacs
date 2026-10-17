//! Reads that would hold the UI up, run on threads of their own and shown once done.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use remacs_core::{CommandResult, Editor};

type Show = Box<dyn FnOnce(&mut Editor) -> CommandResult + Send>;

#[derive(Default)]
pub struct Reads(Vec<Receiver<Show>>);

/// Runs `read` on a thread of its own, saying `busy` meanwhile; [`pick_up`] hands what it
/// read to `show`, or says why it failed.
pub fn spawn<T: Send + 'static>(
    editor: &mut Editor,
    busy: String,
    read: impl FnOnce() -> Result<T, String> + Send + 'static,
    show: impl FnOnce(&mut Editor, T) -> CommandResult + Send + 'static,
) {
    let (send, receive) = mpsc::channel();
    let notifier = editor.notifier();
    thread::Builder::new()
        .name("git-read".into())
        .spawn(move || {
            let shown: Show = match read() {
                Ok(value) => Box::new(move |editor| show(editor, value)),
                Err(error) => Box::new(move |_| Err(error.into())),
            };
            let _ = send.send(shown);
            notifier();
        })
        .expect("spawning a git read thread");
    editor.plugin_state::<Reads>().0.push(receive);
    editor.set_message(busy);
}

/// Shows what finished reads read. Runs after every command.
pub fn pick_up(editor: &mut Editor) {
    let mut done = Vec::new();
    editor
        .plugin_state::<Reads>()
        .0
        .retain(|receiver| match receiver.try_recv() {
            Ok(show) => {
                done.push(show);
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
    for show in done {
        editor.clear_message();
        if let Err(error) = show(editor) {
            editor.set_error(error.0);
        }
    }
}
