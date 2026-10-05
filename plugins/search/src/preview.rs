//! While a search prompt is open, its matches are lit in the buffer and point is on the
//! one chosen, like Emacs isearch; quitting puts point and the view back.

use std::ops::Range;

use remacs_core::{Buffer, Editor, Face, Span};

use crate::query::Query;

/// Matches lit in a buffer, the chosen one apart.
pub struct Lit {
    pub query: Query,
    pub current: Option<Range<usize>>,
}

/// Kept on the searched buffer while its prompt is open.
struct Searching {
    category: &'static str,
    start: usize,
    first_line: usize,
    input: String,
}

pub fn begin(editor: &mut Editor, category: &'static str) {
    let first_line = editor.viewport.first_line;
    let buffer = editor.window_buffer_mut();
    let start = buffer.point();
    buffer.set_local(Searching {
        category,
        start,
        first_line,
        input: String::new(),
    });
}

/// Stops lighting the matches, leaving point where the answer put it.
pub fn end(editor: &mut Editor) {
    let buffer = editor.window_buffer_mut();
    buffer.remove_local::<Searching>();
    buffer.remove_local::<Lit>();
}

/// Follows the prompt's input and selection; once the prompt is gone without an answer,
/// goes back to where the search started. Runs after every command.
pub fn sync(editor: &mut Editor) {
    let Some(category) = editor
        .window_buffer()
        .local::<Searching>()
        .map(|searching| searching.category)
    else {
        return;
    };
    let open = editor
        .minibuffer()
        .filter(|minibuffer| minibuffer.prompt().category == category)
        .map(|minibuffer| {
            let selected = minibuffer
                .selected_candidate()
                .and_then(|candidate| candidate.value.parse::<usize>().ok());
            (minibuffer.input_text(), selected)
        });
    let buffer = editor.window_buffer_mut();
    let Some(searching) = buffer.local_mut::<Searching>() else {
        return;
    };
    let (start, first_line) = (searching.start, searching.first_line);
    let Some((input, selected)) = open else {
        end(editor);
        let buffer = editor.window_buffer_mut();
        buffer.set_point(start);
        editor.viewport.first_line = first_line;
        return;
    };
    if searching.input != input {
        match Query::new(&input) {
            Some(query) => {
                buffer.set_local(Lit {
                    query,
                    current: None,
                });
            }
            None => buffer.remove_local::<Lit>(),
        }
        if let Some(searching) = buffer.local_mut::<Searching>() {
            searching.input = input;
        }
    }
    if let Some(lit) = buffer.local_mut::<Lit>() {
        let len = lit.query.match_len();
        lit.current = selected.map(|offset| offset..offset + len);
    }
    match selected {
        Some(offset) => show(editor, offset),
        None => {
            editor.window_buffer_mut().set_point(start);
            editor.viewport.first_line = first_line;
        }
    }
}

/// Puts point at `offset`, centered when it was off screen.
pub fn show(editor: &mut Editor, offset: usize) {
    let buffer = editor.window_buffer_mut();
    buffer.set_point(offset);
    let line = buffer.point_line();
    if !editor.viewport.contains(line) {
        editor.viewport.center_on(line);
    }
}

/// The matches in `range`, the chosen one apart.
pub fn decorate(buffer: &Buffer, range: Range<usize>) -> Vec<Span> {
    let Some(lit) = buffer.local::<Lit>() else {
        return Vec::new();
    };
    let text = buffer.text();
    let len = lit.query.match_len();
    // A match starting in `range` may end past it.
    let searched = range.start..(range.end + len - 1).min(text.len());
    let mut spans = Vec::new();
    lit.query.find(text, searched, |start| {
        if start >= range.end {
            return false;
        }
        let matched = start..start + len;
        let face = if lit.current.as_ref() == Some(&matched) {
            Face::SearchCurrent
        } else {
            Face::SearchMatch
        };
        spans.push(Span {
            range: matched,
            face,
        });
        true
    });
    spans
}
