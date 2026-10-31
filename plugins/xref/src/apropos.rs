//! Finding symbols anywhere in the project by name, like Emacs `xref-find-apropos`.
//! Backends such as `lsp` take the requests as the name is typed and deliver what
//! matches; the list follows.

use std::mem;
use std::ops::Range;
use std::path::{Path, PathBuf};

use remacs_core::{
    Candidate, CommandResult, Completions, Count, Editor, Prompt, SymbolKind, flex_match,
};
use remacs_files::{FilePreview, project_root};

use crate::definitions::{Location, jump};

pub(crate) const FIND_APROPOS: &str = "xref-find-apropos";
const SYMBOL_CATEGORY: &str = "xref-symbol";

/// What the plugin asks backends: the symbols matching `query` in the project of `from`,
/// the file the search started in, else its directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolRequest {
    pub id: u64,
    pub query: String,
    pub from: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: Option<SymbolKind>,
    /// What it is declared in, such as its type or module.
    pub container: Option<String>,
    pub location: Location,
    pub line: usize,
}

#[derive(Default)]
struct Apropos {
    next_id: u64,
    from: PathBuf,
    /// Paths are shown relative to it.
    root: PathBuf,
    asked: String,
    /// The newest request, which may be answered more than once, e.g. again once a
    /// backend has loaded the project.
    latest: Option<u64>,
    searching: bool,
    requests: Vec<SymbolRequest>,
    /// The latest answer, and the input it was for.
    found: Vec<Symbol>,
    failure: Option<String>,
    found_for: String,
    /// The window shows the symbol selected while the prompt is open.
    preview: Option<FilePreview>,
    shown: Option<Location>,
}

/// Reads a symbol's name and goes to it.
pub(crate) fn find_apropos(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    let from =
        buffer.path().or(buffer.default_directory()).ok_or("No project to search")?.to_path_buf();
    let root =
        project_root(if from.is_dir() { &from } else { from.parent().unwrap_or(Path::new("/")) });
    let preview = FilePreview::begin(editor);
    let state = editor.plugin_state::<Apropos>();
    *state = Apropos {
        next_id: state.next_id,
        from,
        root,
        preview: Some(preview),
        ..Apropos::default()
    };
    let prompt = Prompt {
        label: "Symbol".into(),
        category: SYMBOL_CATEGORY,
        placeholder: "Find symbol in project…",
        require_match: true,
        count: Count::Position,
        wide: true,
        hints: &[("↵", "Go to"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(prompt, symbol_candidates, |editor, answer| {
        let location = answer
            .parse()
            .ok()
            .and_then(|index: usize| {
                let found = &editor.find_plugin_state::<Apropos>()?.found;
                Some(found.get(index)?.location.clone())
            })
            .ok_or("No match")?;
        // Back where the search started, so that `xref-go-back` returns there.
        if let Some(preview) = editor.plugin_state::<Apropos>().preview.take() {
            preview.restore(editor);
            preview.end(editor, Some(&location.path));
        }
        jump(editor, &location)
    })
}

/// Shows the symbol selected and asks the backends again whenever the name typed
/// changes; once the prompt is gone, puts back what the window showed. Runs after every
/// command.
pub(crate) fn update(editor: &mut Editor) {
    let open = editor
        .minibuffer()
        .filter(|minibuffer| minibuffer.prompt().category == SYMBOL_CATEGORY)
        .map(|minibuffer| {
            let selected = minibuffer
                .selected_candidate()
                .and_then(|candidate| candidate.value.parse::<usize>().ok());
            (minibuffer.input_text(), selected)
        });
    let state = editor.plugin_state::<Apropos>();
    let Some((input, selected)) = open else {
        state.latest = None;
        state.shown = None;
        if let Some(preview) = state.preview.take() {
            preview.end(editor, None);
        }
        return;
    };
    let location =
        selected.and_then(|index| state.found.get(index)).map(|symbol| symbol.location.clone());
    show(editor, location);
    let state = editor.plugin_state::<Apropos>();
    let query = input.trim();
    if state.asked == query {
        return;
    }
    state.asked = query.to_owned();
    if query.is_empty() {
        state.latest = None;
        return;
    }
    state.next_id += 1;
    let request =
        SymbolRequest { id: state.next_id, query: query.to_owned(), from: state.from.clone() };
    state.latest = Some(request.id);
    state.searching = true;
    state.requests.push(request);
}

/// Shows `location` in the window, unless it is shown already; with none, what the
/// window showed before.
fn show(editor: &mut Editor, location: Option<Location>) {
    let state = editor.plugin_state::<Apropos>();
    if state.shown == location {
        return;
    }
    state.shown.clone_from(&location);
    let Some(mut preview) = state.preview.take() else {
        return;
    };
    match &location {
        Some(location) => {
            // A file that cannot be read is said when chosen.
            let _ = preview.show_at(editor, &location.path, location.offset);
        }
        None => preview.restore(editor),
    }
    editor.plugin_state::<Apropos>().preview = Some(preview);
}

pub fn take_symbol_requests(editor: &mut Editor) -> Vec<SymbolRequest> {
    mem::take(&mut editor.plugin_state::<Apropos>().requests)
}

/// Answers request `id` with the symbols found, or why none could be looked for; a later
/// answer to the same request replaces it.
pub fn deliver_symbols(editor: &mut Editor, id: u64, found: Result<Vec<Symbol>, String>) {
    let state = editor.plugin_state::<Apropos>();
    if state.latest != Some(id) {
        return;
    }
    state.searching = false;
    (state.found, state.failure) = match found {
        Ok(symbols) => (symbols, None),
        Err(failure) => (Vec::new(), Some(failure)),
    };
    state.found_for.clone_from(&state.asked);
    editor.refresh_minibuffer();
}

/// The symbols found, best first as the backends rank them; while the answer for the
/// latest input is awaited, the last one narrowed to it.
fn symbol_candidates(editor: &Editor, input: &str) -> Completions {
    let Some(state) = editor.find_plugin_state::<Apropos>() else {
        return Completions::default();
    };
    let query = input.trim();
    if query.is_empty() {
        return Completions::default();
    }
    if let Some(failure) = &state.failure {
        return Completions { note: Some(failure.clone()), ..Completions::default() };
    }
    let fresh = state.found_for == query;
    let candidates: Vec<_> = state
        .found
        .iter()
        .enumerate()
        .filter_map(|(index, symbol)| {
            let matches = flex_match(query, &symbol.name);
            if matches.is_none() && !fresh {
                return None;
            }
            Some(candidate(state, index, symbol, matches.unwrap_or_default()))
        })
        .collect();
    let note = match (candidates.is_empty(), state.searching) {
        (true, true) => Some("Searching…".to_owned()),
        (true, false) => Some("No symbols".to_owned()),
        (false, _) => None,
    };
    Completions { note, ..candidates.into() }
}

/// The symbol's name, what it is in, its kind and where.
fn candidate(
    state: &Apropos,
    index: usize,
    symbol: &Symbol,
    matches: Vec<Range<usize>>,
) -> Candidate {
    let path = &symbol.location.path;
    let shown = path.strip_prefix(&state.root).unwrap_or(path);
    Candidate {
        label: symbol.name.clone(),
        matches,
        annotation: symbol.container.clone().unwrap_or_default(),
        tag: symbol.kind.map(SymbolKind::name),
        detail: Some(format!("{}:{}", shown.display(), symbol.line + 1).into()),
        ..Candidate::new(index.to_string())
    }
}
