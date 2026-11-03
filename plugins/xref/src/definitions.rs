use std::mem;
use std::path::{Path, PathBuf};

use remacs_core::{Candidate, CommandResult, Editor, Prompt, filter_candidates};
use remacs_text::{Snapshot, Text};

pub(crate) const FIND_DEFINITIONS: &str = "xref-find-definitions";
pub(crate) const FIND_REFERENCES: &str = "xref-find-references";
pub const FIND_IMPLEMENTATIONS: &str = "xref-find-implementations";
pub const FIND_TYPE_DEFINITIONS: &str = "xref-find-type-definitions";
/// Every lookup's command, whose answers come after it.
const LOOKUPS: [&str; 4] =
    [FIND_DEFINITIONS, FIND_REFERENCES, FIND_IMPLEMENTATIONS, FIND_TYPE_DEFINITIONS];
/// Places to go back to, like `xref-history-storage`'s length.
const MAX_HISTORY: usize = 16;
const MAX_PREVIEW_CHARS: usize = 200;

/// What the plugin asks of the window buffer's backend: where the symbol at `offset` is
/// defined, or used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: u64,
    pub offset: usize,
    pub lookup: Lookup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    Definitions,
    /// Every use, the declaration too.
    References,
    /// Of a trait, an interface or a virtual method.
    Implementations,
    /// The type of the value at point.
    TypeDefinitions,
}

type Hints = &'static [(&'static str, &'static str)];

impl Lookup {
    /// What is looked for, and the label and hints of the list of several.
    #[rustfmt::skip]
    fn words(self) -> (&'static str, &'static str, Hints) {
        match self {
            Lookup::Definitions => ("definitions", "Definitions", &[("↵", "Go to definition"), ("C-g", "Close")]),
            Lookup::References => ("references", "References", &[("↵", "Go to reference"), ("C-g", "Close")]),
            Lookup::Implementations => ("implementations", "Implementations", &[("↵", "Go to implementation"), ("C-g", "Close")]),
            Lookup::TypeDefinitions => ("type definitions", "Type Definitions", &[("↵", "Go to type definition"), ("C-g", "Close")]),
        }
    }
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
        && !editor.last_command().is_some_and(|command| LOOKUPS.contains(&command))
    {
        state(editor).awaited = None;
    }
}

pub(crate) fn find_definitions(editor: &mut Editor) -> CommandResult {
    look_up(editor, Lookup::Definitions)
}

/// Lists every use of the symbol at point, like Emacs `xref-find-references`.
pub(crate) fn find_references(editor: &mut Editor) -> CommandResult {
    look_up(editor, Lookup::References)
}

pub(crate) fn find_implementations(editor: &mut Editor) -> CommandResult {
    look_up(editor, Lookup::Implementations)
}

pub(crate) fn find_type_definitions(editor: &mut Editor) -> CommandResult {
    look_up(editor, Lookup::TypeDefinitions)
}

fn look_up(editor: &mut Editor, lookup: Lookup) -> CommandResult {
    if editor.minibuffer().is_some() {
        return Err("Symbols are looked up in buffers only".into());
    }
    let offset = editor.window_buffer().point();
    let state = state(editor);
    state.next_id += 1;
    let request = Request { id: state.next_id, offset, lookup };
    state.awaited = Some(request);
    state.requests.push(request);
    Ok(())
}

pub(crate) fn go_back(editor: &mut Editor) -> CommandResult {
    let location = state(editor).history.pop().ok_or("At start of xref history")?;
    go_to(editor, &location)
}

pub fn take_requests(editor: &mut Editor) -> Vec<Request> {
    mem::take(&mut state(editor).requests)
}

/// Answers request `id`: goes to the one definition, or lets the user choose among several
/// definitions or any references.
pub fn deliver(editor: &mut Editor, id: u64, mut locations: Vec<Location>) {
    let Some(awaited) = take_awaited(editor, id) else {
        return;
    };
    locations.dedup();
    let result = match (awaited.lookup, locations.as_slice()) {
        (lookup, []) => Err(format!("No {} found", lookup.words().0).into()),
        (Lookup::References, _) => choose(editor, Lookup::References, locations),
        (_, [location]) => jump(editor, location),
        (lookup, _) => choose(editor, lookup, locations),
    };
    if let Err(error) = result {
        editor.set_error(error.0);
    }
}

/// Answers request `id` that `server`, the code's, cannot look up.
pub fn refuse(editor: &mut Editor, id: u64, server: &str) {
    if let Some(awaited) = take_awaited(editor, id) {
        editor.set_error(format!("{server} cannot find {}", awaited.lookup.words().0));
    }
}

fn take_awaited(editor: &mut Editor, id: u64) -> Option<Request> {
    let awaited = &mut state(editor).awaited;
    awaited.take_if(|awaited| awaited.id == id)
}

/// Goes to `location`, remembering where from for `xref-go-back`.
pub(crate) fn jump(editor: &mut Editor, location: &Location) -> CommandResult {
    let buffer = editor.window_buffer();
    let from =
        buffer.path().map(|path| Location { path: path.to_path_buf(), offset: buffer.point() });
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
    editor.show_line(line);
    Ok(())
}

/// Lists the places with their lines, like the `*xref*` buffer.
fn choose(editor: &mut Editor, lookup: Lookup, locations: Vec<Location>) -> CommandResult {
    // A file's places come together, so it is read once and held only while they last.
    let mut last: Option<(&Path, Option<Snapshot>)> = None;
    let candidates: Vec<_> = locations
        .iter()
        .enumerate()
        .map(|(index, location)| {
            if last.as_ref().is_none_or(|(read, _)| *read != location.path) {
                last = Some((&location.path, text_of(editor, &location.path)));
            }
            let text = last.as_ref().and_then(|(_, text)| text.as_ref());
            candidate(text, index, location)
        })
        .collect();
    let (_, label, hints) = lookup.words();
    let prompt = Prompt {
        label: label.into(),
        category: "xref",
        placeholder: "Filter…",
        require_match: true,
        hints,
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

/// The buffer visiting `path`, else the file.
fn text_of(editor: &Editor, path: &Path) -> Option<Snapshot> {
    let open = editor
        .buffers()
        .iter()
        .find(|buffer| buffer.path() == Some(path))
        .map(|buffer| buffer.text().snapshot());
    open.or_else(|| Some(Text::open(path).ok()?.snapshot()))
}

/// The place's line, its number and file.
fn candidate(text: Option<&Snapshot>, index: usize, location: &Location) -> Candidate {
    let (preview, line) =
        text.map_or_else(|| (String::new(), 0), |text| preview(text, location.offset));
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
    let preview = String::from_utf8_lossy(&bytes).trim().chars().take(MAX_PREVIEW_CHARS).collect();
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
