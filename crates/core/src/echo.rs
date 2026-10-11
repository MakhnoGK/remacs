use std::collections::VecDeque;
use std::time::SystemTime;

/// Messages kept for `*Messages*` until the settings say otherwise.
const DEFAULT_LOG_LIMIT: usize = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    Info,
    /// Something asked for was done, e.g. a file written.
    Done,
    Error,
}

/// What the echo area shows, kept in `*Messages*` too.
#[derive(Clone, Debug)]
pub struct EchoMessage {
    pub text: String,
    pub kind: MessageKind,
    /// The command that was running, if any.
    pub command: Option<&'static str>,
    pub at: SystemTime,
    /// Times it came again right after itself, counted instead of kept.
    pub repeats: usize,
}

impl EchoMessage {
    /// The local time it came at, `14:02:37`.
    pub fn clock(&self) -> String {
        let seconds = self
            .at
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()) as libc::time_t;
        // SAFETY: `localtime_r` only writes the `tm` it is given.
        let local = unsafe {
            let mut local = std::mem::zeroed::<libc::tm>();
            libc::localtime_r(&seconds, &mut local);
            local
        };
        format!(
            "{:02}:{:02}:{:02}",
            local.tm_hour, local.tm_min, local.tm_sec
        )
    }
}

/// Every message shown, oldest first, like Emacs `*Messages*`.
pub struct MessageLog {
    messages: VecDeque<EchoMessage>,
    limit: usize,
    /// Changes whenever a message is added or the log is cleared.
    revision: u64,
}

impl Default for MessageLog {
    fn default() -> Self {
        Self {
            messages: VecDeque::new(),
            limit: DEFAULT_LOG_LIMIT,
            revision: 0,
        }
    }
}

impl MessageLog {
    pub fn add(&mut self, message: &EchoMessage) {
        self.revision += 1;
        if let Some(last) = self.messages.back_mut()
            && last.text == message.text
            && last.kind == message.kind
        {
            last.repeats += 1;
            last.at = message.at;
            return;
        }
        self.messages.push_back(message.clone());
        self.trim();
    }

    pub fn messages(&self) -> &VecDeque<EchoMessage> {
        &self.messages
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.revision += 1;
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        self.trim();
    }

    fn trim(&mut self) {
        while self.messages.len() > self.limit {
            self.messages.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(text: &str) -> EchoMessage {
        EchoMessage {
            text: text.to_owned(),
            kind: MessageKind::Info,
            command: None,
            at: SystemTime::now(),
            repeats: 0,
        }
    }

    #[test]
    fn counts_a_message_repeated_right_after_itself() {
        let mut log = MessageLog::default();
        for text in ["a", "a", "b", "a"] {
            log.add(&message(text));
        }
        let kept: Vec<_> = log
            .messages()
            .iter()
            .map(|message| (message.text.as_str(), message.repeats))
            .collect();
        assert_eq!(kept, [("a", 1), ("b", 0), ("a", 0)]);
    }

    #[test]
    fn keeps_only_the_newest_within_the_limit() {
        let mut log = MessageLog::default();
        log.set_limit(2);
        for text in ["a", "b", "c"] {
            log.add(&message(text));
        }
        assert_eq!(log.messages().front().unwrap().text, "b");
        assert_eq!(log.revision(), 3);
    }

    #[test]
    fn reads_the_local_clock() {
        let clock = message("a").clock();
        assert_eq!(clock.len(), 8);
        assert_eq!(clock.as_bytes()[2], b':');
    }
}
