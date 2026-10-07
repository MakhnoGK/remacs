//! A debug session as the run console sees it: output to show, an end, and Stop.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use remacs_core::{Notifier, Tone};
use remacs_run::{Event, Program};

/// What the console and the session share.
#[derive(Default)]
pub struct Shared {
    /// Stop was asked for, with how long the program has to end.
    pub stop: Cell<Option<Duration>>,
    /// How it goes, for the status pill.
    pub state: RefCell<Option<(String, Tone)>>,
}

impl Shared {
    pub fn set_state(&self, text: &str, tone: Tone) {
        *self.state.borrow_mut() = Some((text.to_owned(), tone));
    }
}

pub struct DebugProgram {
    pub events: Receiver<Event>,
    pub shared: Rc<Shared>,
    pub notifier: Notifier,
}

impl Program for DebugProgram {
    fn try_event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    fn stop(&self, timeout: Duration) {
        self.shared.stop.set(Some(timeout));
        (self.notifier)();
    }

    /// The adapter shows the program's output; its input is not passed on.
    fn write_input(&self, _: &[u8]) -> bool {
        false
    }

    fn close_input(&self) {}

    fn state(&self) -> Option<(String, Tone)> {
        self.shared.state.borrow().clone()
    }
}
