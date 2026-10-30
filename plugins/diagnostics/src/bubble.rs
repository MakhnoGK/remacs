use std::cmp::Reverse;
use std::mem;
use std::time::{Duration, Instant};

use remacs_core::{Buffer, Editor, Note, Popup};
use remacs_text::Snapshot;

use crate::buffer_diagnostics::BufferDiagnostics;

/// Point resting on a problem this long shows it; moving through or typing does not.
const DELAY: Duration = Duration::from_millis(500);

#[derive(Default)]
struct State {
    /// `Editor::command_count` when last looked at.
    seen_commands: u64,
    /// The window buffer's text as the last command left it, to tell edits.
    text: Option<Snapshot>,
    /// Where point came to rest after a command that did not edit, and when.
    rest: Option<Rest>,
    /// Point was put on a problem, e.g. by `next-error`: shown without waiting.
    show_now: bool,
    /// Some buffer holds a `Shown`, which the next command removes.
    showing: bool,
}

struct Rest {
    point: usize,
    since: Instant,
}

/// The problems at point are shown while point stays here.
struct Shown {
    point: usize,
}

fn state(editor: &mut Editor) -> &mut State {
    editor.plugin_state()
}

/// Shows the problems at point once the command putting point there is done.
pub(crate) fn show_now(editor: &mut Editor) {
    state(editor).show_now = true;
}

/// Any command hides the problems; one that leaves the text as it was starts the wait
/// for point to rest.
pub(crate) fn update(editor: &mut Editor) {
    let count = editor.command_count();
    if mem::replace(&mut state(editor).seen_commands, count) != count {
        hide(editor);
        let buffer = editor.window_buffer();
        let point = buffer.point();
        let text = buffer.text().snapshot();
        let state = state(editor);
        let edited = state.text.replace(text.clone()).is_some_and(|before| !before.is_same(&text));
        state.rest = (!edited).then(|| Rest { point, since: Instant::now() });
    }
    let point = editor.window_buffer().point();
    let waiting = state(editor);
    let left = waiting
        .rest
        .as_ref()
        .filter(|rest| rest.point == point)
        .map(|rest| DELAY.saturating_sub(rest.since.elapsed()));
    let show_now = mem::take(&mut waiting.show_now);
    if !show_now && left != Some(Duration::ZERO) {
        // Asked again on every refresh: the UI keeps only its earliest timer.
        if let Some(left) = left {
            editor.refresh_after(left);
        }
        return;
    }
    waiting.rest = None;
    // Documentation or a menu asked for at point goes first.
    if editor.minibuffer().is_some() || !editor.popups().is_empty() {
        return;
    }
    editor.window_buffer_mut().set_local(Shown { point });
    state(editor).showing = true;
}

fn hide(editor: &mut Editor) {
    if mem::take(&mut state(editor).showing) {
        for buffer in editor.buffers_mut() {
            buffer.remove_local::<Shown>();
        }
    }
}

/// The problems at point, most severe first, lined up with the earliest one's text.
pub(crate) fn popup(editor: &Editor) -> Option<Popup> {
    if editor.minibuffer().is_some() {
        return None;
    }
    let buffer = editor.window_buffer();
    let point = shown_point(buffer)?;
    let state = buffer.local::<BufferDiagnostics>()?;
    let mut at_point: Vec<_> = state.at(buffer.text(), point).collect();
    let anchor = at_point.iter().map(|(range, _)| range.start).min()?;
    at_point.sort_by_key(|(_, diagnostic)| Reverse(diagnostic.severity));
    Some(Popup {
        anchor,
        notes: at_point
            .into_iter()
            .map(|(_, diagnostic)| Note {
                icon: diagnostic.severity.icon(),
                message: code_quoted(&diagnostic.message),
                source: diagnostic.source.clone(),
            })
            .collect(),
        ..Popup::default()
    })
}

/// Code quoted with backticks, as notes take it. TypeScript and eslint quote it with `'`
/// instead; an apostrophe inside a word, as in `can't`, is no quote.
fn code_quoted(message: &str) -> String {
    let mut quoted = message.to_owned();
    if message.contains('`') {
        return quoted;
    }
    let mut chars = message.char_indices().peekable();
    let mut previous = None;
    let mut open = None;
    while let Some((index, char)) = chars.next() {
        let next = chars.peek().map(|&(_, next)| next);
        let bounds = |char: Option<char>| !char.is_some_and(char::is_alphanumeric);
        match (char, open) {
            ('\n', _) => open = None,
            ('\'', Some(start)) if bounds(next) && previous != Some('\'') => {
                quoted.replace_range(start..start + 1, "`");
                quoted.replace_range(index..index + 1, "`");
                open = None;
            }
            ('\'', None) if bounds(previous) => open = Some(index),
            _ => {}
        }
        previous = Some(char);
    }
    quoted
}

/// Point moved by the mouse hides the problems.
fn shown_point(buffer: &Buffer) -> Option<usize> {
    buffer.local::<Shown>().map(|shown| shown.point).filter(|&point| point == buffer.point())
}

#[cfg(test)]
mod tests {
    use super::code_quoted;

    #[test]
    fn single_quoted_code_turns_into_backticks() {
        assert_eq!(
            code_quoted("Type '\"a\"' is not assignable to type 'number'."),
            "Type `\"a\"` is not assignable to type `number`."
        );
        assert_eq!(
            code_quoted("'x' is assigned a value but never used."),
            "`x` is assigned a value but never used."
        );
    }

    #[test]
    fn apostrophes_and_backticks_stay() {
        assert_eq!(code_quoted("Object can't be 'null'"), "Object can't be `null`");
        let rust = "use of undeclared lifetime name `'a`";
        assert_eq!(code_quoted(rust), rust);
        assert_eq!(code_quoted("it's 'open\nstill"), "it's 'open\nstill");
    }
}
