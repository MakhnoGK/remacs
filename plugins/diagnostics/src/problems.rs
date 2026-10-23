use std::borrow::Cow;
use std::path::Path;

use remacs_core::{
    Buffer, Candidate, CommandResult, Completions, Editor, Prompt, StatusItem, StatusSegment,
    filter_candidates, visiting_file,
};

use crate::bubble;
use crate::buffer_diagnostics::{BufferDiagnostics, Severity};

pub(crate) const LIST_PROBLEMS: &str = "list-problems";
const PROBLEM_CATEGORY: &str = "problem";

/// Lists the buffer's problems, like Emacs `flymake-show-buffer-diagnostics`; closes the
/// list when it is open.
pub(crate) fn list_problems(editor: &mut Editor) -> CommandResult {
    editor.set_next_error_source(None);
    if is_listing(editor) {
        editor.cancel_minibuffer();
        return Ok(());
    }
    let buffer = editor.window_buffer();
    let state = buffer
        .local::<BufferDiagnostics>()
        .ok_or("Nothing checks this buffer")?;
    let point = buffer.point();
    let next = state
        .list
        .iter()
        .find(|diagnostic| diagnostic.range.start >= point)
        .or(state.list.first());
    let prompt = Prompt {
        label: "Problems".into(),
        category: PROBLEM_CATEGORY,
        placeholder: "Filter…",
        default: next.map(|diagnostic| diagnostic.range.start.to_string()),
        require_match: true,
        hints: &[
            ("↵", "Go to problem"),
            ("M-n M-p", "Next / previous"),
            ("C-c ! l", "Toggle list"),
            ("C-g", "Close"),
        ],
        ..Prompt::default()
    };
    editor.read(prompt, problem_candidates, |editor, answer| {
        let offset = answer.parse().map_err(|_| "No match")?;
        go_to(editor, offset);
        Ok(())
    })
}

/// Moves to the next problem, wrapping around, or selects the next one in the list; then
/// `next-error` goes to the problems too.
pub(crate) fn next_problem(editor: &mut Editor) -> CommandResult {
    editor.set_next_error_source(None);
    step(editor, true)
}

pub(crate) fn previous_problem(editor: &mut Editor) -> CommandResult {
    editor.set_next_error_source(None);
    step(editor, false)
}

/// Visits the next location of the source named last, such as the compile output; else
/// moves to the next error or warning, wrapping around, while editing; the list, if open,
/// stays as it is.
pub(crate) fn next_error(editor: &mut Editor) -> CommandResult {
    match editor.next_error_source() {
        Some(source) => (source.next)(editor),
        None => go_to_next(editor, true, Severity::Warning),
    }
}

pub(crate) fn previous_error(editor: &mut Editor) -> CommandResult {
    match editor.next_error_source() {
        Some(source) => (source.previous)(editor),
        None => go_to_next(editor, false, Severity::Warning),
    }
}

/// Whether `next-error` has somewhere to go.
pub(crate) fn has_errors(editor: &Editor) -> bool {
    visiting_file(editor) || editor.next_error_source().is_some()
}

pub(crate) fn status(editor: &Editor) -> Option<StatusSegment> {
    let state = editor.window_buffer().local::<BufferDiagnostics>()?;
    Some(StatusSegment {
        items: Severity::ALL
            .iter()
            .map(|&severity| {
                StatusItem::new(Some(severity.icon()), state.count(severity).to_string())
            })
            .collect(),
        command: Some(LIST_PROBLEMS),
        active: is_listing(editor),
        code: true,
    })
}

fn step(editor: &mut Editor, forward: bool) -> CommandResult {
    if is_listing(editor) {
        let minibuffer = editor.minibuffer_mut().ok_or("Not in the minibuffer")?;
        if forward {
            minibuffer.select_next();
        } else {
            minibuffer.select_previous();
        }
        return Ok(());
    }
    go_to_next(editor, forward, Severity::Info)
}

/// Moves to the next problem at least as severe as `least`, wrapping around.
fn go_to_next(editor: &mut Editor, forward: bool, least: Severity) -> CommandResult {
    let missing = if least == Severity::Info {
        "No problems"
    } else {
        "No errors or warnings"
    };
    let buffer = editor.window_buffer();
    let point = buffer.point();
    let state = buffer.local::<BufferDiagnostics>().ok_or(missing)?;
    let mut starts = state
        .list
        .iter()
        .filter(|diagnostic| diagnostic.severity >= least)
        .map(|diagnostic| diagnostic.range.start);
    let target = if forward {
        starts
            .clone()
            .find(|&start| start > point)
            .or(starts.next())
    } else {
        starts
            .clone()
            .rfind(|&start| start < point)
            .or(starts.next_back())
    };
    go_to(editor, target.ok_or(missing)?);
    Ok(())
}

/// Puts point on the problem at `offset` and shows it there.
fn go_to(editor: &mut Editor, offset: usize) {
    let buffer = editor.window_buffer_mut();
    buffer.set_point(offset);
    buffer.deactivate_mark();
    bubble::show_now(editor);
}

fn is_listing(editor: &Editor) -> bool {
    editor
        .minibuffer()
        .is_some_and(|minibuffer| minibuffer.prompt().category == PROBLEM_CATEGORY)
}

/// One row per problem: its message, source and `line:column` as the annotation.
fn problem_candidates(editor: &Editor, input: &str) -> Completions {
    let buffer = editor.window_buffer();
    let Some(state) = buffer.local::<BufferDiagnostics>() else {
        return Completions::default();
    };
    let group: Cow<str> = group_title(buffer).into();
    let text = buffer.text();
    let candidates = state
        .list
        .iter()
        .map(|diagnostic| {
            let start = diagnostic.range.start.min(text.len());
            Candidate {
                label: diagnostic.summary().to_owned(),
                icon: Some(diagnostic.severity.icon()),
                detail: (!diagnostic.source.is_empty()).then(|| diagnostic.source.clone().into()),
                annotation: format!(
                    "{}:{}",
                    text.line_of(start) + 1,
                    text.char_column(start) + 1
                ),
                group: Some(group.clone()),
                ..Candidate::new(start.to_string())
            }
        })
        .collect();
    filter_candidates(input, candidates, false)
}

/// `buffer.rs  ·  src/`
fn group_title(buffer: &Buffer) -> String {
    let directory = buffer
        .path()
        .and_then(Path::parent)
        .and_then(Path::file_name);
    match directory {
        Some(directory) => format!("{}  ·  {}/", buffer.name(), directory.display()),
        None => buffer.name().to_owned(),
    }
}
