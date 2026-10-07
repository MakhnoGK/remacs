use std::ops::Range;

use memchr::memmem::Finder;
use memchr::{memchr, memrchr};

/// Bytes searched at a time, ending at a line end, so that a file of gigabytes is never
/// copied whole to ignore case.
const CHUNK: usize = 1 << 20;

/// Words that must all be on a line, in any order, each a literal string with smart case
/// like the prompts' filters: ASCII case is ignored unless the input has an uppercase
/// letter.
pub struct Words {
    /// The longest first: the rarest, so the one looked for, the others checked on its
    /// lines.
    finders: Vec<Finder<'static>>,
    ignore_case: bool,
}

impl Words {
    pub fn new(input: &str) -> Option<Self> {
        Self::of(input, input.split_whitespace().collect())
    }

    /// The whole input as one word, spaces included, as text to replace is taken.
    pub fn phrase(input: &str) -> Option<Self> {
        Self::of(
            input,
            (!input.is_empty()).then_some(input).into_iter().collect(),
        )
    }

    fn of(input: &str, words: Vec<&str>) -> Option<Self> {
        let ignore_case = !input.chars().any(char::is_uppercase);
        let mut words: Vec<String> = words
            .into_iter()
            .map(|word| {
                if ignore_case {
                    word.to_ascii_lowercase()
                } else {
                    word.to_owned()
                }
            })
            .collect();
        if words.is_empty() {
            return None;
        }
        words.sort_by_key(|word| std::cmp::Reverse(word.len()));
        words.dedup();
        let finders = words
            .iter()
            .map(|word| Finder::new(word.as_bytes()).into_owned())
            .collect();
        Some(Self {
            finders,
            ignore_case,
        })
    }

    /// Calls `each` with the range of every line of `bytes` that has all the words, its
    /// newline left out, in order, until it returns false. `lowered` holds a chunk's
    /// lowercase copy, reused across calls.
    pub fn lines(
        &self,
        bytes: &[u8],
        lowered: &mut Vec<u8>,
        mut each: impl FnMut(Range<usize>) -> bool,
    ) {
        let (first, rest) = self.finders.split_first().expect("there is a word");
        let mut start = 0;
        while start < bytes.len() {
            let mut end = (start + CHUNK).min(bytes.len());
            if end < bytes.len() {
                end = memchr(b'\n', &bytes[end..]).map_or(bytes.len(), |newline| end + newline + 1);
            }
            let chunk = if self.ignore_case {
                lowered.clear();
                lowered.extend_from_slice(&bytes[start..end]);
                lowered.make_ascii_lowercase();
                &lowered[..]
            } else {
                &bytes[start..end]
            };
            let mut from = 0;
            while let Some(found) = chunk.get(from..).and_then(|rest| first.find(rest)) {
                let at = from + found;
                let line_start = memrchr(b'\n', &chunk[..at]).map_or(0, |newline| newline + 1);
                let line_end =
                    memchr(b'\n', &chunk[at..]).map_or(chunk.len(), |newline| at + newline);
                let line = &chunk[line_start..line_end];
                if rest.iter().all(|word| word.find(line).is_some())
                    && !each(start + line_start..start + line_end)
                {
                    return;
                }
                from = line_end + 1;
            }
            start = end;
        }
    }

    /// Byte ranges of `line` the words match, in order, overlapping ones merged.
    pub fn ranges_in(&self, line: &str) -> Vec<Range<usize>> {
        let haystack = if self.ignore_case {
            line.to_ascii_lowercase()
        } else {
            line.to_owned()
        };
        let mut ranges: Vec<Range<usize>> = self
            .finders
            .iter()
            .flat_map(|finder| {
                let len = finder.needle().len();
                finder
                    .find_iter(haystack.as_bytes())
                    .map(move |start| start..start + len)
            })
            .collect();
        ranges.sort_by_key(|range| range.start);
        let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
        for range in ranges {
            match merged.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => merged.push(range),
            }
        }
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(input: &str, text: &str) -> Vec<String> {
        let words = Words::new(input).unwrap();
        let mut found = Vec::new();
        words.lines(text.as_bytes(), &mut Vec::new(), |range| {
            found.push(text[range].to_owned());
            true
        });
        found
    }

    #[test]
    fn lines_have_every_word_in_any_order_with_smart_case() {
        let text = "fn open_file() {}\nlet file = open();\nFile::open(path)\nopen\n";
        assert_eq!(
            lines("file open", text),
            [
                "fn open_file() {}",
                "let file = open();",
                "File::open(path)"
            ]
        );
        assert_eq!(lines("File open", text), ["File::open(path)"]);
        assert!(Words::new(" \t").is_none());
    }

    #[test]
    fn a_phrase_is_found_whole_spaces_included() {
        let text = "let file = open();\nfile =\nopen file\n";
        let words = Words::phrase("file =").unwrap();
        let mut found = Vec::new();
        words.lines(text.as_bytes(), &mut Vec::new(), |range| {
            found.push(&text[range]);
            true
        });
        assert_eq!(found, ["let file = open();", "file ="]);
        assert!(Words::phrase("").is_none());
    }

    #[test]
    fn lines_across_chunks_are_found_whole() {
        let filler = "x".repeat(CHUNK - 3);
        let text = format!("{filler}\nneedle at the edge\nlast needle");
        assert_eq!(
            lines("needle", &text),
            ["needle at the edge", "last needle"]
        );
    }

    #[test]
    fn ranges_of_every_word_are_merged() {
        let words = Words::new("ab bc x").unwrap();
        assert_eq!(words.ranges_in("ABC x ab"), [0..3, 4..5, 6..8]);
    }
}
