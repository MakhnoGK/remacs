//! While `project-search` is open, the window shows the selected line in its file, the
//! words lit, like Emacs consult's preview; quitting puts the buffer, point and view
//! back, and the files opened only to be shown are closed.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use remacs_core::{Buffer, CommandResult, Editor, Face, Span};

use crate::words::Words;

/// Lit lines longer than this are lit only this far, as they are drawn every frame.
const MAX_LIT_LINE: usize = 4096;

/// A found line: the file, from the project's root, its line and the bytes to its first
/// match.
pub struct Place<'a> {
    pub file: &'a str,
    pub line: usize,
    pub column: usize,
}

impl<'a> Place<'a> {
    /// What a candidate stands for, `file:line:column`.
    pub fn parse(value: &'a str) -> Option<Self> {
        let mut parts = value.rsplitn(3, ':');
        let column = parts.next()?.parse().ok()?;
        let line = parts.next()?.parse().ok()?;
        Some(Self {
            file: parts.next()?,
            line,
            column,
        })
    }

    pub fn value(file: &str, line: usize, column: usize) -> String {
        format!("{file}:{line}:{column}")
    }
}

/// Visits the file, point on the match, centered when off screen.
pub fn go_to(editor: &mut Editor, path: &Path, place: &Place) -> CommandResult {
    editor.visit_file(path)?;
    let buffer = editor.window_buffer_mut();
    let range = buffer.text().line_range(place.line).unwrap_or_default();
    buffer.deactivate_mark();
    buffer.set_point((range.start + place.column).min(range.end));
    let line = buffer.point_line();
    if !editor.viewport.contains(line) {
        editor.viewport.center_on(line);
    }
    Ok(())
}

/// What the window showed before, and what the preview opened since.
#[derive(Default)]
pub struct Preview {
    origin: Option<Origin>,
    /// Files that had no buffer before being shown.
    opened: Vec<PathBuf>,
    /// The candidate shown.
    shown: Option<String>,
}

struct Origin {
    buffer: String,
    point: usize,
    first_line: usize,
}

/// Kept on the buffer shown: the words, and the line chosen.
struct Lit {
    words: Arc<Words>,
    line: usize,
}

impl Preview {
    pub fn begin(editor: &Editor) -> Self {
        let buffer = editor.window_buffer();
        Self {
            origin: Some(Origin {
                buffer: buffer.name().to_owned(),
                point: buffer.point(),
                first_line: editor.viewport.first_line,
            }),
            ..Self::default()
        }
    }

    /// Shows the candidate `value` stands for, unless it is shown already; with none,
    /// what the window showed before.
    pub fn show(
        &mut self,
        editor: &mut Editor,
        root: &Path,
        value: Option<&str>,
        words: &Arc<Words>,
    ) {
        if self.shown.as_deref() == value {
            return;
        }
        self.shown = value.map(str::to_owned);
        unlight(editor);
        let Some(place) = value.and_then(Place::parse) else {
            return self.restore(editor);
        };
        let path = root.join(place.file);
        if !editor
            .buffers()
            .iter()
            .any(|buffer| buffer.path() == Some(path.as_path()))
        {
            self.opened.push(path.clone());
        }
        if go_to(editor, &path, &place).is_err() {
            return;
        }
        editor.window_buffer_mut().set_local(Lit {
            words: Arc::clone(words),
            line: place.line,
        });
    }

    /// Shows what the window showed before, as there is nothing to search for.
    pub fn show_nothing(&mut self, editor: &mut Editor) {
        if self.shown.take().is_some() {
            unlight(editor);
            self.restore(editor);
        }
    }

    /// Puts back what the window showed unless a line was `chosen`, and closes the files
    /// opened only to be shown.
    pub fn end(mut self, editor: &mut Editor, chosen: Option<&Path>) {
        unlight(editor);
        if chosen.is_none() {
            self.restore(editor);
        }
        for path in self.opened {
            if Some(path.as_path()) == chosen {
                continue;
            }
            let name = editor
                .buffers()
                .iter()
                .find(|buffer| buffer.path() == Some(path.as_path()))
                .map(|buffer| buffer.name().to_owned());
            if let Some(name) = name {
                editor.kill_buffer(&name);
            }
        }
    }

    fn restore(&mut self, editor: &mut Editor) {
        let Some(origin) = &self.origin else {
            return;
        };
        if editor.switch_to_buffer(&origin.buffer) {
            editor.window_buffer_mut().set_point(origin.point);
            editor.viewport.first_line = origin.first_line;
        }
    }
}

fn unlight(editor: &mut Editor) {
    for buffer in editor.buffers_mut() {
        buffer.remove_local::<Lit>();
    }
}

/// The words on the lines in `range` of the buffer shown, the chosen line's apart.
pub fn decorate(buffer: &Buffer, range: Range<usize>) -> Vec<Span> {
    let Some(lit) = buffer.local::<Lit>() else {
        return Vec::new();
    };
    let text = buffer.text();
    let mut spans = Vec::new();
    for line in text.line_of(range.start)..=text.line_of(range.end) {
        let Some(line_range) = text.line_range(line) else {
            break;
        };
        let end = line_range.end.min(line_range.start + MAX_LIT_LINE);
        let line_text = String::from_utf8_lossy(&text.bytes_in(line_range.start..end)).into_owned();
        let face = if line == lit.line {
            Face::SearchCurrent
        } else {
            Face::SearchMatch
        };
        spans.extend(
            lit.words
                .ranges_in(&line_text)
                .into_iter()
                .map(|matched| Span {
                    range: line_range.start + matched.start..line_range.start + matched.end,
                    face,
                }),
        );
    }
    spans
}
