use std::ops::Range;

use memchr::memmem::Finder;
use remacs_text::Snapshot;

/// A literal string to find, with smart case like the prompts' filters: ASCII case is
/// ignored unless it has an uppercase letter.
pub struct Query {
    finder: Finder<'static>,
    ignore_case: bool,
    len: usize,
}

impl Query {
    pub fn new(text: &str) -> Option<Self> {
        if text.is_empty() {
            return None;
        }
        let ignore_case = !text.chars().any(char::is_uppercase);
        let needle = if ignore_case {
            text.to_ascii_lowercase()
        } else {
            text.to_owned()
        };
        Some(Self {
            finder: Finder::new(needle.as_bytes()).into_owned(),
            ignore_case,
            len: text.len(),
        })
    }

    /// Calls `each` with the start of every match in `text`, in order, until it returns
    /// false. Reads a chunk at a time, so a file of gigabytes is never copied whole.
    pub fn find(&self, text: &Snapshot, mut each: impl FnMut(usize) -> bool) {
        // Enough of the previous chunk for a match that crosses into the next.
        let overlap = self.len - 1;
        let mut window = Vec::new();
        let mut window_start = 0;
        let mut next_allowed = 0;
        for chunk in text.chunks(0..text.len()) {
            let kept = window.len().min(overlap);
            window_start += window.len() - kept;
            window.drain(..window.len() - kept);
            window.extend_from_slice(chunk);
            if self.ignore_case {
                window[kept..].make_ascii_lowercase();
            }
            for found in self.finder.find_iter(&window) {
                let start = window_start + found;
                // A match in the kept bytes may overlap one already reported.
                if start < next_allowed {
                    continue;
                }
                next_allowed = start + self.len;
                if !each(start) {
                    return;
                }
            }
        }
    }

    /// Byte ranges of the matches in `line`.
    pub fn ranges_in(&self, line: &str) -> Vec<Range<usize>> {
        let haystack = if self.ignore_case {
            line.to_ascii_lowercase()
        } else {
            line.to_owned()
        };
        self.finder
            .find_iter(haystack.as_bytes())
            .map(|start| start..start + self.len)
            .collect()
    }
}
