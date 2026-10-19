//! Searching the text of the whole project, the matching lines listed by file in a prompt
//! like the other searches, like Emacs `consult-ripgrep`.

use std::mem;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use remacs_core::{Candidate, CommandResult, Completions, Count, Editor, Prompt, Section};
use remacs_files::{
    FileIndex, abbreviate_home, default_directory, project_files, project_root, with_slash,
};
use serde::Deserialize;

use crate::project_preview::{Place, Preview, go_to};
use crate::scan::{Job, Scan};
use crate::words::Words;

pub const PROJECT_SEARCH: &str = "project-search";
const CATEGORY: &str = "project-search";

/// `search.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct SearchSettings {
    /// Lines listed by `project-search` at most; the search stops there.
    pub project_max_matches: usize,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            project_max_matches: 10_000,
        }
    }
}

impl Section for SearchSettings {
    const NAME: &str = "search";

    fn check(&self) -> Result<(), String> {
        match self.project_max_matches {
            0 => Err("project-max-matches is 0".into()),
            _ => Ok(()),
        }
    }
}

#[derive(Default)]
struct ProjectSearch {
    /// Of the prompt it lists lines for.
    category: &'static str,
    /// The input is one string to find, spaces included, rather than words.
    phrase: bool,
    root: PathBuf,
    index: Option<Arc<FileIndex>>,
    max_lines: usize,
    /// What the running search is for.
    input: String,
    files: Option<Arc<Vec<String>>>,
    /// The files searched are all the project's, not those cached or none while its
    /// first scan runs.
    all_files: bool,
    words: Option<Arc<Words>>,
    scan: Option<Scan>,
    preview: Preview,
    /// The file of the line chosen, which stays open.
    chosen: Option<PathBuf>,
}

/// Reads words and lists the project's lines that have them all, grouped by file, the
/// window showing the one selected; choosing one goes to its first match.
pub fn project_search(editor: &mut Editor) -> CommandResult {
    let prompt = Prompt {
        label: "Search project".into(),
        category: CATEGORY,
        placeholder: "Words to find…",
        require_match: true,
        hints: &[("↵", "Go to"), ("C-x C-q", "Edit"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    read_lines(editor, prompt, false, |editor, answer| {
        let place = Place::parse(&answer).ok_or("No match")?;
        let state = editor.plugin_state::<ProjectSearch>();
        let path = state.root.join(place.file);
        state.chosen = Some(path.clone());
        go_to(editor, &path, &place)
    })
}

/// Opens `prompt` listing the project's lines that have what is typed, grouped by file,
/// the window showing the one selected: every word, or with `phrase`, the whole input.
/// The project's root is shown by the input.
pub fn read_lines(
    editor: &mut Editor,
    prompt: Prompt,
    phrase: bool,
    on_answer: impl FnOnce(&mut Editor, String) -> CommandResult + 'static,
) -> CommandResult {
    let root = project_root(&default_directory(editor));
    let index = project_files(editor, &root);
    let max_lines = editor
        .settings_section::<SearchSettings>()
        .project_max_matches;
    let prompt = Prompt {
        directory: with_slash(&abbreviate_home(&root)),
        count: Count::Position,
        wide: true,
        ..prompt
    };
    let preview = Preview::begin(editor);
    *editor.plugin_state::<ProjectSearch>() = ProjectSearch {
        category: prompt.category,
        phrase,
        root,
        index: Some(index),
        max_lines,
        preview,
        ..ProjectSearch::default()
    };
    editor.read(prompt, candidates, on_answer)
}

/// The project searched by the open prompt, its files, and what was typed.
pub fn searched(editor: &Editor) -> Option<(PathBuf, Arc<FileIndex>, String)> {
    let state = editor.find_plugin_state::<ProjectSearch>()?;
    let index = Arc::clone(state.index.as_ref()?);
    Some((state.root.clone(), index, state.input.clone()))
}

/// Whether the open prompt lists the project's lines.
pub fn is_open(editor: &Editor) -> bool {
    editor
        .find_plugin_state::<ProjectSearch>()
        .filter(|state| state.index.is_some())
        .is_some_and(|state| {
            editor
                .minibuffer()
                .is_some_and(|minibuffer| minibuffer.prompt().category == state.category)
        })
}

/// The numbers of the lines found, by file from the project's root.
pub type FoundLines = Vec<(String, Vec<usize>)>;

/// Closes the prompt, putting back what the window showed; the project's root and the
/// lines found, unless the search is still running.
pub fn close(editor: &mut Editor) -> Result<(PathBuf, FoundLines), &'static str> {
    let state = editor.find_plugin_state::<ProjectSearch>();
    let scan = state
        .and_then(|state| state.scan.as_ref())
        .ok_or("Nothing found")?;
    if !scan.found().done {
        return Err("Still searching; try again in a moment");
    }
    editor.cancel_minibuffer();
    let state = mem::take(editor.plugin_state::<ProjectSearch>());
    state.preview.end(editor, None);
    let found = state.scan.as_ref().map(|scan| {
        scan.found()
            .files
            .values()
            .map(|(file, hits)| (file.clone(), hits.iter().map(|hit| hit.line).collect()))
            .collect()
    });
    Ok((state.root, found.unwrap_or_default()))
}

/// Searches again whenever the input or the project's files change, stopping the last
/// search, and shows the line selected; once the prompt is gone, stops and ends the
/// preview. Runs after every command.
pub fn update(editor: &mut Editor) {
    let Some(category) = editor
        .find_plugin_state::<ProjectSearch>()
        .filter(|state| state.index.is_some())
        .map(|state| state.category)
    else {
        return;
    };
    let open = editor
        .minibuffer()
        .filter(|minibuffer| minibuffer.prompt().category == category)
        .map(|minibuffer| {
            let selected = minibuffer
                .selected_candidate()
                .map(|candidate| candidate.value.clone());
            (minibuffer.input_text(), selected)
        });
    let Some((input, selected)) = open else {
        let state = mem::take(editor.plugin_state::<ProjectSearch>());
        state.preview.end(editor, state.chosen.as_deref());
        return;
    };
    search(editor, input);
    let state = editor.plugin_state::<ProjectSearch>();
    let (root, mut preview) = (state.root.clone(), mem::take(&mut state.preview));
    match state.words.clone() {
        Some(words) => preview.show(editor, &root, selected.as_deref(), &words),
        None => preview.show_nothing(editor),
    }
    editor.plugin_state::<ProjectSearch>().preview = preview;
}

/// Starts searching for `input` unless it is searched for in the project's files as they
/// are now.
fn search(editor: &mut Editor, input: String) {
    let Some(state) = editor.find_plugin_state::<ProjectSearch>() else {
        return;
    };
    let Some((files, all_files)) = state.index.as_ref().map(|index| index.files_and_scanned())
    else {
        return;
    };
    let same_files = state
        .files
        .as_ref()
        .is_some_and(|searched| Arc::ptr_eq(searched, &files));
    if state.input == input && same_files {
        return;
    }
    let root = state.root.clone();
    let unsaved = unsaved_buffers(editor, &root);
    let notifier = editor.notifier();
    let state = editor.plugin_state::<ProjectSearch>();
    let words = if state.phrase {
        Words::phrase(&input)
    } else {
        Words::new(&input)
    };
    state.words = words.map(Arc::new);
    state.scan = state.words.as_ref().map(|words| {
        let job = Job {
            root,
            files: Arc::clone(&files),
            unsaved,
            words: Arc::clone(words),
            max_lines: state.max_lines,
            max_file_lines: usize::MAX,
        };
        Scan::start(job, notifier)
    });
    state.input = input;
    state.files = Some(files);
    state.all_files = all_files;
}

/// Buffers of the project's files with unsaved changes, by path from `root`.
pub fn unsaved_buffers(editor: &Editor, root: &Path) -> Vec<(String, remacs_text::Snapshot)> {
    editor
        .buffers()
        .iter()
        .filter(|buffer| buffer.is_modified())
        .filter_map(|buffer| {
            let relative = buffer.path()?.strip_prefix(root).ok()?;
            Some((relative.to_str()?.to_owned(), buffer.text().snapshot()))
        })
        .collect()
}

/// The lines found so far, under their files in the order searched.
fn candidates(editor: &Editor, input: &str) -> Completions {
    let Some(state) = editor.find_plugin_state::<ProjectSearch>() else {
        return Completions::default();
    };
    let Some(scan) = &state.scan else {
        return Completions::default();
    };
    let found = scan.found();
    let mut candidates = Vec::with_capacity(found.lines);
    for (file, hits) in found.files.values() {
        for hit in hits {
            candidates.push(Candidate {
                label: hit.label.clone(),
                matches: hit.matches.clone(),
                annotation: (hit.line + 1).to_string(),
                group: Some(file.clone().into()),
                ..Candidate::new(Place::value(file, hit.line, hit.column))
            });
        }
    }
    let done = found.done && (state.all_files || found.more);
    let note = match (done, found.more, candidates.is_empty()) {
        (false, _, _) => Some("Searching…".to_owned()),
        (true, true, _) => Some(format!("First {} lines", found.lines)),
        (true, false, true) if !input.trim().is_empty() => Some("No matches".to_owned()),
        _ => None,
    };
    Completions {
        note,
        count_groups: true,
        ..candidates.into()
    }
}
