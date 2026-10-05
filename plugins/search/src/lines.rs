use remacs_core::{Buffer, Candidate, CommandResult, Completions, Count, Editor, Keymap, Prompt};
use remacs_text::Snapshot;

use crate::query::Query;

pub const SEARCH: &str = "search-buffer";
const SEARCH_CATEGORY: &str = "search";
/// Beyond this many lines the rest are left out, so that a common string in a huge file
/// still answers each key at once.
const MAX_LINES: usize = 10_000;
/// Bytes of a line shown; a longer one shows the part around its first match.
const MAX_LABEL: usize = 240;
const CONTEXT_BEFORE_MATCH: usize = 40;

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
            go_to(editor, offset, start);
            Ok(())
        },
    )
}

/// The lines containing `input`, each valued at its first match.
fn line_candidates(buffer: &Buffer, input: &str, point: usize) -> Completions {
    let Some(query) = Query::new(input) else {
        return Completions::default();
    };
    let text = buffer.text();
    let mut candidates = Vec::new();
    let mut starts = Vec::new();
    let mut last_line = None;
    let mut more = false;
    query.find(text, |start| {
        let line = text.line_of(start);
        if last_line == Some(line) {
            return true;
        }
        if candidates.len() == MAX_LINES {
            more = true;
            return false;
        }
        last_line = Some(line);
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

fn candidate(text: &Snapshot, query: &Query, line: usize, start: usize) -> Candidate {
    let range = text.line_range(line).unwrap_or_default();
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
    let label = String::from_utf8_lossy(&text.bytes_in(from..to))
        .trim()
        .to_owned();
    Candidate {
        matches: query.ranges_in(&label),
        label,
        annotation: (line + 1).to_string(),
        ..Candidate::new(start.to_string())
    }
}

/// Puts point on the match, centered when it was off screen, and the mark where the
/// search started, like Emacs isearch.
fn go_to(editor: &mut Editor, offset: usize, start: usize) {
    let buffer = editor.window_buffer_mut();
    buffer.set_mark(start);
    buffer.deactivate_mark();
    buffer.set_point(offset);
    let line = buffer.point_line();
    if !editor.viewport.contains(line) {
        editor.viewport.center_on(line);
    }
}
