//! Choosing what to run: the project's configurations, then what it can run that has
//! none yet, grouped by kind.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use remacs_core::{
    Candidate, CommandResult, Completions, Count, Editor, Icon, Keymap, Prompt, filter_candidates,
    quote,
};
use remacs_files::{FileIndex, project_files};

use crate::commands::{edit_configuration, new_configuration, run_configuration, stop_session};
use crate::configuration::Configuration;
use crate::configuration_store;
use crate::discovery::discover;
use crate::launch::launch;
use crate::process::Starter;
use crate::sessions::{self, Session, Sessions};
use crate::window::window_root;

pub const EDIT: &str = "run-chooser-edit";
const HINTS: &[(&str, &str)] = &[("↵", "Run"), ("C-e", "Edit"), ("C-g", "Cancel")];

/// The open chooser's list, shared with its prompt.
#[derive(Default)]
struct Chooser {
    listing: Option<Rc<RefCell<Listing>>>,
}

/// The configurations listed, by candidate value, with what the project can run found
/// again whenever its file index changes, e.g. as its first scan ends.
struct Listing {
    root: PathBuf,
    saved: Vec<Configuration>,
    temporary: Option<Configuration>,
    index: Arc<FileIndex>,
    /// The index's files `entries` were found in.
    files: Option<Arc<Vec<String>>>,
    scanned: bool,
    entries: Vec<Entry>,
}

#[derive(Clone)]
struct Entry {
    configuration: Configuration,
    origin: Origin,
}

/// Where a configuration listed comes from, in the order listed within a kind.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    Saved,
    Temporary,
    /// Found in the project, with no configuration yet.
    Found,
}

/// The candidate value of "New configuration…", apart from the entries' indexes.
const NEW: &str = "new";

impl Listing {
    fn update(&mut self) {
        let (files, scanned) = self.index.files_and_scanned();
        self.scanned = scanned;
        if self
            .files
            .as_ref()
            .is_some_and(|known| Arc::ptr_eq(known, &files))
        {
            return;
        }
        let found = discover(&self.root, &files)
            .into_iter()
            .filter(|found| !self.saved.iter().any(|saved| saved.kind == found.kind));
        let entry = |origin| {
            move |configuration| Entry {
                configuration,
                origin,
            }
        };
        let mut entries: Vec<Entry> = self
            .saved
            .iter()
            .cloned()
            .map(entry(Origin::Saved))
            .chain(self.temporary.clone().map(entry(Origin::Temporary)))
            .chain(found.map(entry(Origin::Found)))
            .collect();
        entries.sort_by_key(|entry| (entry.configuration.kind.group_rank(), entry.origin));
        self.entries = entries;
        self.files = Some(files);
    }

    fn completions(&self, editor: &Editor, input: &str) -> Completions {
        let candidates = self.candidates(editor);
        let mut completions = filter_candidates(input, candidates, false);
        completions.count_groups = true;
        completions.subject = Some(format!(
            "{} configurations · {}",
            self.saved.len(),
            quote(&self.root.file_name().unwrap_or_default().to_string_lossy())
        ));
        if !self.scanned {
            completions.note = Some("Looking through the project…".into());
        }
        completions
    }

    fn candidates(&self, editor: &Editor) -> Vec<Candidate> {
        let settings = editor
            .find_plugin_state::<Sessions>()
            .map(|sessions| sessions.settings.clone())
            .unwrap_or_default();
        let new = Candidate {
            value: NEW.into(),
            icon: Some(Icon::Create),
            detail: Some("an example to fill in, in the run file".into()),
            ..Candidate::new("New configuration…")
        };
        self.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let configuration = &entry.configuration;
                let running = sessions::find(editor, &self.root, &configuration.name)
                    .and_then(|id| sessions::get(editor, id))
                    .is_some_and(Session::is_running);
                let command_line = launch(configuration, &self.root, &settings)
                    .map_or_else(|error| error, |launch| launch.command_line());
                Candidate {
                    value: index.to_string(),
                    label: configuration.name.clone(),
                    verb: match entry.origin {
                        Origin::Saved => None,
                        Origin::Temporary => Some("Temporary"),
                        Origin::Found => Some("New"),
                    },
                    detail: Some(command_line.into()),
                    tag: running.then_some("running"),
                    group: Some(configuration.kind.group().into()),
                    icon: Some(configuration.kind.icon()),
                    dim: entry.origin != Origin::Saved,
                    ..Candidate::default()
                }
            })
            .chain([new])
            .collect()
    }
}

pub fn choose(editor: &mut Editor) -> CommandResult {
    choose_with(editor, None)
}

/// Asks which configuration of the window buffer's project to run, and runs it, or starts
/// it with `starter`.
pub fn choose_with(editor: &mut Editor, starter: Option<Starter>) -> CommandResult {
    let root = window_root(editor).ok_or("Not in a project")?;
    let (project, error) = configuration_store(editor).project(&root);
    let saved = project.configurations.clone();
    let temporary = project.temporary.clone();
    if let Some(error) = error {
        editor.set_error(error);
    }
    let index = project_files(editor, &root);
    let listing = Rc::new(RefCell::new(Listing {
        root,
        saved,
        temporary,
        index,
        files: None,
        scanned: false,
        entries: Vec::new(),
    }));
    editor.plugin_state::<Chooser>().listing = Some(Rc::clone(&listing));
    let mut keymap = Keymap::default();
    keymap.bind("C-e", EDIT);
    let prompt = Prompt {
        label: "Run configuration".into(),
        count: Count::Matches,
        require_match: true,
        wide: true,
        hints: HINTS,
        keymap: Some(keymap),
        ..Prompt::default()
    };
    editor.read(
        prompt,
        move |editor, input| {
            let mut listing = listing.borrow_mut();
            listing.update();
            listing.completions(editor, input)
        },
        |editor, value| {
            let (root, entry) = take_entry(editor, &value)?;
            let Some(entry) = entry else {
                return new_configuration(editor, &root);
            };
            let configuration = match entry.origin {
                Origin::Saved | Origin::Temporary => entry.configuration,
                Origin::Found => {
                    configuration_store(editor).find_or_add(&root, entry.configuration)?
                }
            };
            run_configuration(editor, &root, configuration, starter)
        },
    )
}

/// The project and the entry chosen; none for "New configuration…".
fn take_entry(editor: &mut Editor, value: &str) -> Result<(PathBuf, Option<Entry>), String> {
    let listing = editor
        .plugin_state::<Chooser>()
        .listing
        .take()
        .ok_or("No such configuration")?;
    let listing = listing.borrow();
    if value == NEW {
        return Ok((listing.root.clone(), None));
    }
    let index: usize = value.parse().map_err(|_| "No such configuration")?;
    let entry = listing
        .entries
        .get(index)
        .cloned()
        .ok_or("No such configuration")?;
    Ok((listing.root.clone(), Some(entry)))
}

/// Edits the selected candidate's configuration, saved first if it is not yet.
pub fn edit(editor: &mut Editor) -> CommandResult {
    let value = editor
        .minibuffer()
        .and_then(|minibuffer| minibuffer.selected_candidate())
        .map(|candidate| candidate.value.clone())
        .ok_or("No configuration")?;
    editor.cancel_minibuffer();
    match take_entry(editor, &value)? {
        (root, Some(entry)) => edit_configuration(editor, &root, entry.configuration),
        (root, None) => new_configuration(editor, &root),
    }
}

/// Asks which of the `running` sessions to stop.
pub fn choose_running(editor: &mut Editor, running: Vec<(u64, String)>) -> CommandResult {
    let candidates: Vec<Candidate> = running
        .iter()
        .map(|(id, name)| Candidate {
            value: id.to_string(),
            ..Candidate::new(name.clone())
        })
        .collect();
    let prompt = Prompt {
        label: "Stop".into(),
        require_match: true,
        ..Prompt::default()
    };
    editor.read(
        prompt,
        move |_, input| filter_candidates(input, candidates.clone(), false),
        |editor, value| {
            let id = value.parse().map_err(|_| "No such run")?;
            stop_session(editor, id)
        },
    )
}
