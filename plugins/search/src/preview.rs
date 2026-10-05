//! While the search prompt is open, its matches are lit in the buffer and point is on the
//! one chosen, like Emacs isearch; quitting puts point and the view back.

use std::ops::Range;

use remacs_core::{Buffer, Editor, Face, Span};

use crate::query::Query;

/// Kept on the searched buffer while its prompt is open.
pub struct Searching {
    category: &'static str,
    start: usize,
    first_line: usize,
    input: String,
    query: Option<Query>,
    current: Option<Range<usize>>,
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
        query: None,
        current: None,
    });
}

/// Stops lighting the matches, leaving point where the answer put it.
pub fn end(editor: &mut Editor) {
    editor.window_buffer_mut().remove_local::<Searching>();
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
        buffer.remove_local::<Searching>();
        buffer.set_point(start);
        editor.viewport.first_line = first_line;
        return;
    };
    if searching.input != input {
        searching.query = Query::new(&input);
        searching.input = input;
    }
    let len = searching.query.as_ref().map_or(0, Query::match_len);
    searching.current = selected.map(|offset| offset..offset + len);
    match selected {
        Some(offset) => {
            buffer.set_point(offset);
            let line = buffer.point_line();
            if !editor.viewport.contains(line) {
                editor.viewport.center_on(line);
            }
        }
        None => {
            buffer.set_point(start);
            editor.viewport.first_line = first_line;
        }
    }
}

/// The matches in `range`, the chosen one apart.
pub fn decorate(buffer: &Buffer, range: Range<usize>) -> Vec<Span> {
    let Some(searching) = buffer.local::<Searching>() else {
        return Vec::new();
    };
    let Some(query) = &searching.query else {
        return Vec::new();
    };
    let text = buffer.text();
    // A match starting in `range` may end past it.
    let searched = range.start..(range.end + query.match_len() - 1).min(text.len());
    let mut spans = Vec::new();
    query.find(text, searched, |start| {
        if start >= range.end {
            return false;
        }
        let matched = start..start + query.match_len();
        let face = if searching.current.as_ref() == Some(&matched) {
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
