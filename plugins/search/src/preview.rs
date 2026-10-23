//! While a search prompt is open, its matches are lit in the buffer and point is on the
//! one chosen, like Emacs isearch; quitting puts point and the view back.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use remacs_core::{Buffer, Editor, Face, Span};

use crate::query::Query;

/// Matches lit in a buffer, the one chosen, by where it starts, apart.
pub struct Lit {
    pub query: Query,
    pub current: Option<usize>,
}

/// Kept on the searched buffer while its prompt is open.
struct Searching {
    category: &'static str,
    start: usize,
    first_line: usize,
    /// The prompt's regexp switch.
    regex: Rc<Cell<bool>>,
    /// What is lit: the input, and whether it is a regular expression.
    lit: (String, bool),
}

pub fn begin(editor: &mut Editor, category: &'static str, regex: &Rc<Cell<bool>>) {
    let first_line = editor.viewport.first_line;
    let buffer = editor.window_buffer_mut();
    let start = buffer.point();
    buffer.set_local(Searching {
        category,
        start,
        first_line,
        regex: Rc::clone(regex),
        lit: (String::new(), regex.get()),
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
    let lit = (input, searching.regex.get());
    if searching.lit != lit {
        match Query::new(&lit.0, lit.1) {
            Ok(Some(query)) => {
                buffer.set_local(Lit {
                    query,
                    current: None,
                });
            }
            _ => buffer.remove_local::<Lit>(),
        }
        if let Some(searching) = buffer.local_mut::<Searching>() {
            searching.lit = lit;
        }
    }
    if let Some(lit) = buffer.local_mut::<Lit>() {
        lit.current = selected;
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
    editor.show_line(line);
}

/// The matches in `range`, the chosen one apart.
pub fn decorate(buffer: &Buffer, range: Range<usize>) -> Vec<Span> {
    let Some(lit) = buffer.local::<Lit>() else {
        return Vec::new();
    };
    let mut spans = Vec::new();
    lit.query.find(buffer.text(), range, |matched| {
        let face = if lit.current == Some(matched.start) {
            Face::SearchCurrent
        } else {
            Face::SearchMatch
        };
        if !matched.is_empty() {
            spans.push(Span {
                range: matched,
                face,
            });
        }
        true
    });
    spans
}
