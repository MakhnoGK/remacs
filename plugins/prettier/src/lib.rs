//! Built-in plugin: a format backend that formats web files of projects using prettier
//! with their own prettier, else the global one. Registered before `lsp`, which formats
//! what it leaves.

mod project;
mod worker;

use std::rc::Rc;

use remacs_core::{Editor, Plugin, Registry};

use crate::project::uses_prettier;
use crate::worker::Worker;

pub struct PrettierPlugin {
    /// Major modes whose files prettier formats.
    modes: &'static [&'static str],
}

impl PrettierPlugin {
    pub const fn new(modes: &'static [&'static str]) -> Self {
        Self { modes }
    }
}

impl Plugin for PrettierPlugin {
    fn register(&self, registry: &mut Registry) {
        let modes = self.modes;
        registry
            .post_command_hooks
            .push(Rc::new(move |editor| sync(editor, modes)));
    }
}

/// Started on the first format.
#[derive(Default)]
struct State {
    worker: Option<Worker>,
}

/// Hands formatted texts over and sends the format plugin's new requests.
fn sync(editor: &mut Editor, modes: &'static [&'static str]) {
    let formatted = editor
        .plugin_state::<State>()
        .worker
        .as_ref()
        .map(Worker::take_formatted)
        .unwrap_or_default();
    for (id, formatted) in formatted {
        remacs_format::deliver(editor, id, formatted);
    }
    let requests = remacs_format::take_requests(editor, |request| {
        modes.contains(&request.mode) && uses_prettier(&request.path)
    });
    for request in requests {
        let id = request.id;
        if let Err(message) = send(editor, request) {
            remacs_format::deliver(editor, id, Err(message));
        }
    }
}

/// Sends to the worker, starting it, or a new one if it stopped.
fn send(editor: &mut Editor, request: remacs_format::Request) -> Result<(), String> {
    let notifier = editor.notifier();
    let state = editor.plugin_state::<State>();
    if let Some(worker) = &state.worker
        && worker.send(request.clone()).is_ok()
    {
        return Ok(());
    }
    let worker = Worker::start(notifier).map_err(|error| format!("node: {error}"))?;
    let sent = worker.send(request);
    state.worker = Some(worker);
    sent.map_err(|()| "prettier stopped".into())
}
