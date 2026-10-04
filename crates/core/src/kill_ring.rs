use std::collections::VecDeque;

/// Like Emacs `kill-ring-max`.
const MAX_ENTRIES: usize = 120;

/// Killed text, newest first, shared by all buffers and synced with the system clipboard.
#[derive(Default)]
pub struct KillRing {
    entries: VecDeque<String>,
    /// Entry the last yank inserted, like Emacs `kill-ring-yank-pointer`.
    yank_index: usize,
    /// The newest kill has not been handed to the system clipboard yet.
    unexported: bool,
}

impl KillRing {
    pub fn push(&mut self, text: String) {
        self.push_entry(text);
        self.unexported = true;
    }

    /// Extends the newest entry, so that consecutive kills yank back as one.
    pub fn append(&mut self, text: &str, before: bool) {
        match self.entries.front_mut() {
            Some(newest) if before => newest.insert_str(0, text),
            Some(newest) => newest.push_str(text),
            None => self.push_entry(text.to_owned()),
        }
        self.yank_index = 0;
        self.unexported = true;
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(String::as_str)
    }

    /// The newest entry; it resets where `rotate` starts from.
    pub fn newest(&mut self) -> Option<&str> {
        self.yank_index = 0;
        self.entries.front().map(String::as_str)
    }

    /// The entry after the one yanked last, wrapping around like Emacs `yank-pop`.
    pub fn rotate(&mut self) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        self.yank_index = (self.yank_index + 1) % self.entries.len();
        self.entries.get(self.yank_index).map(String::as_str)
    }

    /// The newest kill, once, for the system clipboard.
    pub fn take_export(&mut self) -> Option<String> {
        if !std::mem::take(&mut self.unexported) {
            return None;
        }
        self.entries.front().cloned()
    }

    /// Text another program put on the clipboard becomes the newest kill.
    pub fn receive(&mut self, text: String) {
        if !text.is_empty() && self.entries.front() != Some(&text) {
            self.push_entry(text);
        }
    }

    fn push_entry(&mut self, text: String) {
        self.entries.push_front(text);
        self.entries.truncate(MAX_ENTRIES);
        self.yank_index = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_through_kills_and_exports_each_once() {
        let mut ring = KillRing::default();
        ring.push("one".into());
        ring.push("two".into());
        ring.append(" more", false);
        assert_eq!(ring.take_export().as_deref(), Some("two more"));
        assert_eq!(ring.take_export(), None);
        assert_eq!(ring.newest(), Some("two more"));
        assert_eq!(ring.rotate(), Some("one"));
        assert_eq!(ring.rotate(), Some("two more"));
        ring.receive("two more".into());
        ring.receive("pasted".into());
        assert_eq!(
            ring.entries().collect::<Vec<_>>(),
            ["pasted", "two more", "one"]
        );
        assert_eq!(ring.take_export(), None);
    }
}
