use std::mem;
use std::path::{Path, PathBuf};

use remacs_core::{Candidate, CommandResult, Editor, Prompt, filter_candidates};
use remacs_text::{Snapshot, Text};

pub(crate) const FIND_DEFINITIONS: &str = "xref-find-definitions";
/// Places to go back to, like `xref-history-storage`'s length.
const MAX_HISTORY: usize = 16;
const MAX_PREVIEW_CHARS: usize = 200;

/// What the plugin asks of the window buffer's backend: where the symbol at `offset` is
/// defined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: u64,
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub offset: usize,
}

#[derive(Default)]
struct State {
    next_id: u64,
    awaited: Option<Request>,
    requests: Vec<Request>,
    /// Where definitions were looked up from, newest last.
    history: Vec<Location>,
    /// `Editor::command_count` when last looked at.
    seen_commands: u64,
}

fn state(editor: &mut Editor) -> &mut State {
    editor.plugin_state()
}

/// An answer that comes after another command is dropped, so point never jumps away
/// from what the user went on to do.
pub(crate) fn update(editor: &mut Editor) {
    let count = editor.command_count();
    if mem::replace(&mut state(editor).seen_commands, count) != count
        && editor.last_command() != Some(FIND_DEFINITIONS)
    {
        state(editor).awaited = None;
    }
}

pub(crate) fn find_definitions(editor: &mut Editor) -> CommandResult {
    if editor.minibuffer().is_some() {
        return Err("Definitions are looked up in buffers only".into());
    }
    let offset = editor.window_buffer().point();
    let state = state(editor);
    state.next_id += 1;
    let request = Request {
        id: state.next_id,
        offset,
    };
    state.awaited = Some(request);
    state.requests.push(request);
    Ok(())
}

pub(crate) fn go_back(editor: &mut Editor) -> CommandResult {
    let location = state(editor)
        .history
        .pop()
        .ok_or("At start of xref history")?;
    go_to(editor, &location)
}

pub fn take_requests(editor: &mut Editor) -> Vec<Request> {
    mem::take(&mut state(editor).requests)
}

/// Answers request `id`: goes to the one definition, or lets the user choose among several.
pub fn deliver(editor: &mut Editor, id: u64, mut locations: Vec<Location>) {
    let state = state(editor);
    if state.awaited.is_none_or(|awaited| awaited.id != id) {
        return;
    }
    state.awaited = None;
    locations.dedup();
    let result = match locations.as_slice() {
        [] => Err("No definitions found".into()),
        [location] => jump(editor, location),
        _ => choose(editor, locations),
    };
    if let Err(error) = result {
        editor.set_error(error.0);
    }
}

/// Goes to `location`, remembering where from for `xref-go-back`.
pub(crate) fn jump(editor: &mut Editor, location: &Location) -> CommandResult {
    let buffer = editor.window_buffer();
    let from = buffer.path().map(|path| Location {
        path: path.to_path_buf(),
        offset: buffer.point(),
    });
    go_to(editor, location)?;
    let history = &mut state(editor).history;
    history.extend(from);
    if history.len() > MAX_HISTORY {
        history.remove(0);
    }
    Ok(())
}

/// Visits the file and puts point on `location`, centered when it was off screen. Answers
/// arrive outside commands, so nothing else scrolls to it.
fn go_to(editor: &mut Editor, location: &Location) -> CommandResult {
    editor
        .visit_file(&location.path)
        .map_err(|error| format!("{}: {error}", location.path.display()))?;
    let buffer = editor.window_buffer_mut();
    buffer.set_point(location.offset);
    buffer.deactivate_mark();
    let line = buffer.point_line();
    if !editor.viewport.contains(line) {
        editor.viewport.center_on(line);
    }
    Ok(())
}

/// Lists the definitions with their lines, like the `*xref*` buffer.
fn choose(editor: &mut Editor, locations: Vec<Location>) -> CommandResult {
    let candidates: Vec<_> = locations
        .iter()
        .enumerate()
        .map(|(index, location)| candidate(editor, index, location))
        .collect();
    let prompt = Prompt {
        label: "Definitions".into(),
        category: "xref",
        placeholder: "Filter…",
        require_match: true,
        hints: &[("↵", "Go to definition"), ("C-g", "Close")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        move |_, input| filter_candidates(input, candidates.clone(), false),
        move |editor, answer| {
            let location = answer
                .parse()
                .ok()
                .and_then(|index: usize| locations.get(index))
                .ok_or("No match")?;
            jump(editor, location)
        },
    )
}

/// The definition's line, its number and file.
fn candidate(editor: &Editor, index: usize, location: &Location) -> Candidate {
    let open = editor
        .buffers()
        .iter()
        .find(|buffer| buffer.path() == Some(&location.path))
        .map(|buffer| buffer.text().snapshot());
    let text = open.or_else(|| Some(Text::open(&location.path).ok()?.snapshot()));
    let (preview, line) = text
        .as_ref()
        .map_or_else(|| (String::new(), 0), |text| preview(text, location.offset));
    Candidate {
        label: preview,
        annotation: (line + 1).to_string(),
        group: Some(file_title(&location.path).into()),
        ..Candidate::new(index.to_string())
    }
}

fn preview(text: &Snapshot, offset: usize) -> (String, usize) {
    let line = text.line_of(offset.min(text.len()));
    let range = text.line_range(line).unwrap_or_default();
    let bytes = text.bytes_in(range);
    let preview = String::from_utf8_lossy(&bytes)
        .trim()
        .chars()
        .take(MAX_PREVIEW_CHARS)
        .collect();
    (preview, line)
}

/// `buffer.rs  ·  src/`
fn file_title(path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    match path.parent().and_then(Path::file_name) {
        Some(directory) => format!("{name}  ·  {}/", directory.display()),
        None => name.into_owned(),
    }
}
