//! Going to a symbol of the file by name, like Emacs `imenu`: backends such as `lsp` take
//! the request and deliver the file's symbols, which the list then narrows down. The
//! window shows the symbol selected.

use std::mem;
use std::path::PathBuf;

use remacs_core::{
    Candidate, CommandResult, Completions, Count, Editor, Prompt, SymbolKind, flex_match,
};
use remacs_files::FilePreview;

use crate::apropos::Symbol;

pub(crate) const IMENU: &str = "imenu";
const CATEGORY: &str = "imenu";

/// What the plugin asks backends: the symbols of the file at `path`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentSymbolRequest {
    pub id: u64,
    pub path: PathBuf,
}

#[derive(Default)]
struct Imenu {
    next_id: u64,
    latest: Option<u64>,
    requests: Vec<DocumentSymbolRequest>,
    /// The answer, once delivered.
    found: Option<Result<Vec<Symbol>, String>>,
    preview: Option<FilePreview>,
    shown: Option<usize>,
}

/// Reads the name of one of the file's symbols and goes to it.
pub(crate) fn imenu(editor: &mut Editor) -> CommandResult {
    let path = editor
        .window_buffer()
        .path()
        .ok_or("Buffer is not visiting a file")?
        .to_path_buf();
    let preview = FilePreview::begin(editor);
    let state = editor.plugin_state::<Imenu>();
    state.next_id += 1;
    let request = DocumentSymbolRequest {
        id: state.next_id,
        path,
    };
    *state = Imenu {
        next_id: state.next_id,
        latest: Some(request.id),
        requests: vec![request],
        preview: Some(preview),
        ..Imenu::default()
    };
    let prompt = Prompt {
        label: "Symbol".into(),
        category: CATEGORY,
        placeholder: "Go to symbol in file…",
        require_match: true,
        count: Count::Position,
        hints: &[("↵", "Go to"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(prompt, candidates, |editor, answer| {
        let offset = answer
            .parse()
            .ok()
            .and_then(|index: usize| offset_of(editor.find_plugin_state::<Imenu>()?, index))
            .ok_or("No match")?;
        if let Some(preview) = editor.plugin_state::<Imenu>().preview.take() {
            preview.restore(editor);
        }
        let buffer = editor.window_buffer_mut();
        buffer.push_mark();
        buffer.set_point(offset);
        buffer.deactivate_mark();
        let line = buffer.point_line();
        if !editor.viewport.contains(line) {
            editor.viewport.center_on(line);
        }
        Ok(())
    })
}

/// Shows the symbol selected; once the prompt is gone, puts back what the window showed
/// unless a symbol was chosen. Runs after every command.
pub(crate) fn update(editor: &mut Editor) {
    let open = editor
        .minibuffer()
        .filter(|minibuffer| minibuffer.prompt().category == CATEGORY)
        .map(|minibuffer| {
            minibuffer
                .selected_candidate()
                .and_then(|candidate| candidate.value.parse::<usize>().ok())
        });
    let state = editor.plugin_state::<Imenu>();
    let Some(selected) = open else {
        state.latest = None;
        state.shown = None;
        if let Some(preview) = state.preview.take() {
            preview.end(editor, None);
        }
        return;
    };
    if state.shown == selected {
        return;
    }
    state.shown = selected;
    let offset = selected.and_then(|index| offset_of(state, index));
    let Some(mut preview) = state.preview.take() else {
        return;
    };
    match offset {
        Some(offset) => {
            let path = editor.window_buffer().path().map(PathBuf::from);
            if let Some(path) = path {
                let _ = preview.show_at(editor, &path, offset);
            }
        }
        None => preview.restore(editor),
    }
    editor.plugin_state::<Imenu>().preview = Some(preview);
}

fn offset_of(state: &Imenu, index: usize) -> Option<usize> {
    let symbols = state.found.as_ref()?.as_ref().ok()?;
    Some(symbols.get(index)?.location.offset)
}

pub fn take_document_symbol_requests(editor: &mut Editor) -> Vec<DocumentSymbolRequest> {
    mem::take(&mut editor.plugin_state::<Imenu>().requests)
}

/// Answers request `id` with the file's symbols, or why none could be looked for.
pub fn deliver_document_symbols(editor: &mut Editor, id: u64, found: Result<Vec<Symbol>, String>) {
    let state = editor.plugin_state::<Imenu>();
    if state.latest != Some(id) {
        return;
    }
    state.found = Some(found);
    editor.refresh_minibuffer();
}

/// The symbols matching `input`, in the order the backends listed them.
fn candidates(editor: &Editor, input: &str) -> Completions {
    let found = editor
        .find_plugin_state::<Imenu>()
        .and_then(|state| state.found.as_ref());
    let symbols = match found {
        None => return note("Asking the language server…"),
        Some(Err(failure)) => return note(failure),
        Some(Ok(symbols)) => symbols,
    };
    let query = input.trim();
    let candidates: Vec<_> = symbols
        .iter()
        .enumerate()
        .filter_map(|(index, symbol)| {
            let matches = if query.is_empty() {
                Vec::new()
            } else {
                flex_match(query, &symbol.name)?
            };
            Some(Candidate {
                label: symbol.name.clone(),
                matches,
                annotation: symbol.container.clone().unwrap_or_default(),
                tag: symbol.kind.map(SymbolKind::name),
                detail: Some(format!("{}", symbol.line + 1).into()),
                ..Candidate::new(index.to_string())
            })
        })
        .collect();
    if candidates.is_empty() {
        return note("No symbols");
    }
    candidates.into()
}

fn note(text: &str) -> Completions {
    Completions {
        note: Some(text.to_owned()),
        ..Completions::default()
    }
}
