use std::time::SystemTime;

use super::Editor;
use crate::echo::{EchoMessage, MessageKind, MessageLog};
use crate::key::describe_sequence;

impl Editor {
    /// The echo area as text: the message, or the prefix typed so far (`C-x-`).
    pub fn echo(&self) -> Option<String> {
        if self.pending_keys.is_empty() {
            self.message.as_ref().map(|message| message.text.clone())
        } else {
            Some(format!("{}-", describe_sequence(&self.pending_keys)))
        }
    }

    /// The echo area's message, shown until the next key.
    pub fn message(&self) -> Option<&EchoMessage> {
        self.message.as_ref()
    }

    /// Takes the message out of the echo area before the next key; `*Messages*` keeps it.
    pub fn clear_message(&mut self) {
        self.message = None;
    }

    pub fn set_message(&mut self, text: impl Into<String>) {
        self.show_message(text.into(), MessageKind::Info);
    }

    /// A message saying something asked for was done, e.g. a file written.
    pub fn set_done(&mut self, text: impl Into<String>) {
        self.show_message(text.into(), MessageKind::Done);
    }

    pub fn set_error(&mut self, text: impl Into<String>) {
        self.show_message(text.into(), MessageKind::Error);
    }

    /// Every message shown, for `*Messages*`.
    pub fn message_log(&self) -> &MessageLog {
        &self.messages
    }

    pub fn message_log_mut(&mut self) -> &mut MessageLog {
        &mut self.messages
    }

    /// Lists all the view's keys, not only those that fit under it, until the next key.
    pub fn list_key_hints(&mut self) {
        self.key_hints_listed = true;
    }

    pub fn key_hints_listed(&self) -> bool {
        self.key_hints_listed
    }

    fn show_message(&mut self, text: String, kind: MessageKind) {
        let message = EchoMessage {
            text,
            kind,
            command: self.this_command,
            at: SystemTime::now(),
            repeats: 0,
        };
        self.messages.add(&message);
        self.message = Some(message);
    }

    /// What the echo area showed for the last key is gone with the next one.
    pub(super) fn clear_echo(&mut self) {
        self.message = None;
        self.key_hints_listed = false;
    }
}
