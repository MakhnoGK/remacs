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
    let root = project_root(&default_directory(editor));
    let index = project_files(editor, &root);
    let max_lines = editor
        .settings_section::<SearchSettings>()
        .project_max_matches;
    let prompt = Prompt {
        label: "Search project".into(),
        category: CATEGORY,
        prefix: with_slash(&abbreviate_home(&root)),
        placeholder: "Words to find…",
        require_match: true,
        count: Count::Position,
        wide: true,
        hints: &[("↵", "Go to"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    let preview = Preview::begin(editor);
    *editor.plugin_state::<ProjectSearch>() = ProjectSearch {
        root,
        index: Some(index),
        max_lines,
        preview,
        ..ProjectSearch::default()
    };
    editor.read(prompt, candidates, |editor, answer| {
        let place = Place::parse(&answer).ok_or("No match")?;
        let state = editor.plugin_state::<ProjectSearch>();
        let path = state.root.join(place.file);
        state.chosen = Some(path.clone());
        go_to(editor, &path, &place)
    })
}

/// Searches again whenever the words typed or the project's files change, stopping the
/// last search, and shows the line selected; once the prompt is gone, stops and ends the
/// preview. Runs after every command.
pub fn update(editor: &mut Editor) {
    let open = editor
        .minibuffer()
        .filter(|minibuffer| minibuffer.prompt().category == CATEGORY)
        .map(|minibuffer| {
            let selected = minibuffer
                .selected_candidate()
                .map(|candidate| candidate.value.clone());
            (minibuffer.input_text(), selected)
        });
    let Some((input, selected)) = open else {
        if editor
            .find_plugin_state::<ProjectSearch>()
            .is_some_and(|state| state.index.is_some())
        {
            let state = mem::take(editor.plugin_state::<ProjectSearch>());
            state.preview.end(editor, state.chosen.as_deref());
        }
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
    state.words = Words::new(&input).map(Arc::new);
    state.scan = state.words.as_ref().map(|words| {
        let job = Job {
            root,
            files: Arc::clone(&files),
            unsaved,
            words: Arc::clone(words),
            max_lines: state.max_lines,
        };
        Scan::start(job, notifier)
    });
    state.input = input;
    state.files = Some(files);
    state.all_files = all_files;
}

/// Buffers of the project's files with unsaved changes, by path from `root`.
fn unsaved_buffers(editor: &Editor, root: &Path) -> Vec<(String, remacs_text::Snapshot)> {
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
