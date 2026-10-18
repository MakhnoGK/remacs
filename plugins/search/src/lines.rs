use std::ops::Range;

use memchr::{memchr, memchr_iter, memrchr};
use remacs_core::{Buffer, Candidate, CommandResult, Completions, Count, Editor, Keymap, Prompt};
use remacs_text::Snapshot;

use crate::preview;
use crate::query::Query;

pub const SEARCH: &str = "search-buffer";
const SEARCH_CATEGORY: &str = "search";
/// Beyond this many lines the rest are left out, so that a common string in a huge file
/// still answers each key at once.
const MAX_LINES: usize = 10_000;
/// Bytes of a line shown; a longer one shows the part around its first match.
const MAX_LABEL: usize = 240;
const CONTEXT_BEFORE_MATCH: usize = 40;
/// Past this many bytes to the next match, its line is looked up rather than counted.
const MAX_COUNTED_GAP: usize = 64 * 1024;

/// Reads a string and lists the lines that contain it, the first match after point
/// selected; choosing one goes to its first match.
pub fn search(editor: &mut Editor) -> CommandResult {
    let start = editor.window_buffer().point();
    let mut keymap = Keymap::default();
    keymap.bind("C-s", "next-candidate");
    keymap.bind("C-r", "previous-candidate");
    let prompt = Prompt {
        label: "Search".into(),
        category: SEARCH_CATEGORY,
        placeholder: "Find in buffer…",
        require_match: true,
        count: Count::Position,
        hints: &[
            ("↵", "Go to"),
            ("C-s C-r", "Next / previous"),
            ("C-g", "Cancel"),
        ],
        keymap: Some(keymap),
        ..Prompt::default()
    };
    editor.read(
        prompt,
        move |editor, input| line_candidates(editor.window_buffer(), input, start),
        move |editor, answer| {
            let offset = answer.parse().map_err(|_| "No match")?;
            preview::end(editor);
            go_to(editor, offset, start);
            Ok(())
        },
    )?;
    preview::begin(editor, SEARCH_CATEGORY);
    Ok(())
}

/// The lines containing `input`, each valued at its first match.
fn line_candidates(buffer: &Buffer, input: &str, point: usize) -> Completions {
    let Some(query) = Query::new(input) else {
        return Completions::default();
    };
    let text = buffer.text();
    let mut candidates = Vec::new();
    let mut starts = Vec::new();
    let mut lines = LineCursor::new(text);
    // The end of the last line listed; later matches before it are on it too.
    let mut listed_end = None;
    let mut more = false;
    query.find(text, 0..text.len(), |start| {
        if listed_end.is_some_and(|end| start < end) {
            return true;
        }
        if candidates.len() == MAX_LINES {
            more = true;
            return false;
        }
        let line = lines.line_at(start);
        listed_end = Some(line.range.end);
        starts.push(start);
        candidates.push(candidate(text, &query, line, start));
        true
    });
    Completions {
        total: candidates.len(),
        selected: starts.iter().position(|&start| start >= point),
        note: more.then(|| format!("First {MAX_LINES} lines")),
        candidates,
        ..Completions::default()
    }
}

/// A line: its number and its bytes without the newline.
struct Line {
    number: usize,
    range: Range<usize>,
}

/// Lines of offsets asked for in order, counted in one pass over the text: the line
/// index looks lines up only to its 16 KiB block and then counts, which for a match
/// on every line of a big file costs more than the search.
struct LineCursor<'a> {
    text: &'a Snapshot,
    /// Newlines before here are counted.
    counted_to: usize,
    line: usize,
    line_start: usize,
}

impl<'a> LineCursor<'a> {
    fn new(text: &'a Snapshot) -> Self {
        Self {
            text,
            counted_to: 0,
            line: 0,
            line_start: 0,
        }
    }

    /// The line of `offset`, at or after the last one asked for.
    fn line_at(&mut self, offset: usize) -> Line {
        if offset - self.counted_to > MAX_COUNTED_GAP {
            self.line = self.text.line_of(offset);
            self.line_start = self.text.line_start(self.line).unwrap_or(0);
            self.counted_to = offset;
        }
        let mut chunk_start = self.counted_to;
        for chunk in self.text.chunks(self.counted_to..offset) {
            self.line += memchr_iter(b'\n', chunk).count();
            if let Some(newline) = memrchr(b'\n', chunk) {
                self.line_start = chunk_start + newline + 1;
            }
            chunk_start += chunk.len();
        }
        let mut end = offset;
        for chunk in self.text.chunks(offset..self.text.len()) {
            match memchr(b'\n', chunk) {
                Some(newline) => {
                    end += newline;
                    break;
                }
                None => end += chunk.len(),
            }
        }
        // No newline before the line's end.
        self.counted_to = end;
        Line {
            number: self.line,
            range: self.line_start..end,
        }
    }
}

fn candidate(text: &Snapshot, query: &Query, line: Line, start: usize) -> Candidate {
    let Line { number, range } = line;
    let boundary = |offset| {
        if text.is_char_boundary(offset) {
            offset
        } else {
            text.previous_char_boundary(offset)
        }
    };
    let from = if range.len() > MAX_LABEL {
        boundary(start.saturating_sub(CONTEXT_BEFORE_MATCH).max(range.start))
    } else {
        range.start
    };
    let to = boundary((from + MAX_LABEL).min(range.end));
    let label = match String::from_utf8(text.bytes_in(from..to)) {
        Ok(label) if label.trim().len() == label.len() => label,
        Ok(label) => label.trim().to_owned(),
        Err(invalid) => String::from_utf8_lossy(invalid.as_bytes())
            .trim()
            .to_owned(),
    };
    Candidate {
        matches: query.ranges_in(&label),
        label,
        annotation: (number + 1).to_string(),
        value: start.to_string(),
        ..Candidate::default()
    }
}

/// Puts point on the match, centered when it was off screen, and pushes the mark where
/// the search started, like Emacs isearch.
fn go_to(editor: &mut Editor, offset: usize, start: usize) {
    let buffer = editor.window_buffer_mut();
    buffer.set_point(start);
    buffer.push_mark();
    buffer.deactivate_mark();
    buffer.set_point(offset);
    let line = buffer.point_line();
    if !editor.viewport.contains(line) {
        editor.viewport.center_on(line);
    }
}
