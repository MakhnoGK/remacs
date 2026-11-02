//! Choosing what to run: the project's configurations, then what it can run that has
//! none yet, grouped by kind.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use remacs_core::{
    Candidate, CommandResult, Completions, Count, Editor, Icon, Keymap, Notifier, Prompt,
    filter_candidates, quote,
};
use remacs_files::{FileIndex, project_files};

use crate::commands::{edit_configuration, new_configuration, run_configuration, stop_session};
use crate::configuration::{Configuration, Project};
use crate::configuration_store;
use crate::discovery::discover;
use crate::launch::launch;
use crate::process::Starter;
use crate::sessions::{self, Session, Sessions};
use crate::settings::RunSettings;
use crate::window::window_root;

pub const EDIT: &str = "run-chooser-edit";
const HINTS: &[(&str, &str)] = &[("↵", "Run"), ("C-e", "Edit"), ("C-g", "Cancel")];

/// The open chooser's list, shared with its prompt.
#[derive(Default)]
pub(crate) struct Chooser {
    listing: Option<Rc<RefCell<Listing>>>,
    /// Each project's search for what it can run, kept while the chooser is closed: one
    /// opened again lists at once what was found, and quick openings start no search
    /// while one is going on.
    pub(crate) searches: HashMap<PathBuf, Rc<RefCell<Search>>>,
}

/// What a project can run, found on a thread of its own: reading every manifest of a
/// big monorepo takes too long to wait for. Found again whenever the file index changes,
/// e.g. as its first scan ends, one search at a time.
#[derive(Default)]
pub(crate) struct Search {
    /// The index's files `found` was, or is being, looked for in.
    files: Option<Arc<Vec<String>>>,
    pub(crate) running: Option<Receiver<Vec<Entry>>>,
    found: Arc<Vec<Entry>>,
    /// Counts the times `found` was replaced, so that a listing knows it is behind.
    pub(crate) generation: u64,
    /// Why the last search failed, until another starts; shown in place of what it would
    /// have found.
    failed: Option<String>,
}

impl Search {
    /// Takes what the search going on found, and starts one for files the index lists
    /// since.
    fn update(
        &mut self,
        root: &Path,
        files: Arc<Vec<String>>,
        settings: &RunSettings,
        notifier: Notifier,
    ) {
        if let Some(running) = &self.running {
            match running.try_recv() {
                Ok(found) => {
                    self.found = Arc::new(found);
                    self.generation += 1;
                    self.running = None;
                }
                Err(TryRecvError::Empty) => return,
                // It panicked; tried again once the index changes.
                Err(TryRecvError::Disconnected) => {
                    self.running = None;
                    self.failed =
                        Some("Could not look through the project for what it runs".into());
                    return;
                }
            }
        }
        if self.files.as_ref().is_some_and(|known| Arc::ptr_eq(known, &files)) {
            return;
        }
        let (send, receive) = mpsc::channel();
        let (root, settings, searched) = (root.to_path_buf(), settings.clone(), Arc::clone(&files));
        let spawned = thread::Builder::new().name("run-discovery".into()).spawn(move || {
            let found = discover(&root, &searched)
                .into_iter()
                .map(|found| Entry::new(found, Origin::Found, &root, &settings))
                .collect();
            // The chooser may be closed by now.
            if send.send(found).is_ok() {
                notifier();
            }
        });
        self.failed = None;
        match spawned {
            Ok(_) => (self.running, self.files) = (Some(receive), Some(files)),
            Err(error) => {
                self.failed = Some(format!("Could not look through the project: {error}"))
            }
        }
    }
}

/// The configurations listed, by candidate value: the saved ones, then what the project's
/// search found that none of them runs.
struct Listing {
    root: PathBuf,
    saved: Vec<Configuration>,
    temporary: Option<Configuration>,
    index: Arc<FileIndex>,
    search: Rc<RefCell<Search>>,
    scanned: bool,
    searching: bool,
    failed: Option<String>,
    /// The generation of the project's search `entries` list.
    listed: Option<u64>,
    entries: Vec<Entry>,
}

#[derive(Clone)]
pub(crate) struct Entry {
    configuration: Configuration,
    origin: Origin,
    /// The command line it runs, else why it cannot run; found once per listing, as it
    /// looks at files, e.g. for a package manager's lock file.
    command_line: Arc<str>,
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

impl Entry {
    fn new(
        configuration: Configuration,
        origin: Origin,
        root: &Path,
        settings: &RunSettings,
    ) -> Self {
        let command_line = launch(&configuration, root, settings)
            .map_or_else(|error| error, |launch| launch.command_line());
        Entry { configuration, origin, command_line: command_line.into() }
    }
}

impl Listing {
    fn new(
        root: PathBuf,
        project: &Project,
        index: Arc<FileIndex>,
        search: Rc<RefCell<Search>>,
    ) -> Self {
        Listing {
            saved: project.configurations.clone(),
            temporary: project.temporary.clone(),
            index,
            search,
            scanned: false,
            searching: false,
            failed: None,
            listed: None,
            entries: Vec::new(),
            root,
        }
    }

    /// Brings the project's search up to date and lists what it found, if it is new.
    fn update(&mut self, editor: &Editor) {
        let (files, scanned) = self.index.files_and_scanned();
        self.scanned = scanned;
        let settings = run_settings(editor);
        let mut search = self.search.borrow_mut();
        search.update(&self.root, files, &settings, editor.notifier());
        self.searching = search.running.is_some();
        self.failed = search.failed.clone();
        let (generation, found) = (search.generation, Arc::clone(&search.found));
        drop(search);
        if self.listed != Some(generation) {
            self.list(&found, &settings);
            self.listed = Some(generation);
        }
    }

    /// Lists the saved and temporary configurations, then what was `found` that none of
    /// them runs.
    fn list(&mut self, found: &[Entry], settings: &RunSettings) {
        let root = &self.root;
        let saved = &self.saved;
        let unsaved = found
            .iter()
            .filter(|found| !saved.iter().any(|saved| saved.kind == found.configuration.kind));
        let mut entries: Vec<Entry> = self
            .saved
            .iter()
            .map(|saved| (saved, Origin::Saved))
            .chain(self.temporary.iter().map(|temporary| (temporary, Origin::Temporary)))
            .map(|(configuration, origin)| {
                Entry::new(configuration.clone(), origin, root, settings)
            })
            .chain(unsaved.cloned())
            .collect();
        entries.sort_by_key(|entry| (entry.configuration.kind.group_rank(), entry.origin));
        self.entries = entries;
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
        if let Some(failed) = &self.failed {
            completions.note = Some(failed.clone());
        } else if !self.scanned || self.searching {
            completions.note = Some("Looking through the project…".into());
        }
        completions
    }

    fn candidates(&self, editor: &Editor) -> Vec<Candidate> {
        let new = Candidate {
            value: NEW.into(),
            icon: Some(Icon::Create),
            detail: Some("made from what runs here, to edit in the run file".into()),
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
                Candidate {
                    value: index.to_string(),
                    label: configuration.name.clone(),
                    verb: match entry.origin {
                        Origin::Saved => None,
                        Origin::Temporary => Some("Temporary"),
                        Origin::Found => Some("New"),
                    },
                    detail: Some(entry.command_line.to_string().into()),
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
    let index = project_files(editor, &root);
    let search =
        Rc::clone(editor.plugin_state::<Chooser>().searches.entry(root.clone()).or_default());
    let (project, error) = configuration_store(editor).project(&root);
    let mut listing = Listing::new(root, project, index, search);
    if let Some(error) = error {
        editor.set_error(error);
    }
    listing.update(editor);
    let listing = Rc::new(RefCell::new(listing));
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
            listing.update(editor);
            listing.completions(editor, input)
        },
        |editor, value| {
            let (root, entry) = match take_entry(editor, &value)? {
                (root, Chosen::Entry(entry)) => (root, entry),
                (root, Chosen::New(first)) => return new_configuration(editor, &root, first),
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

/// What was chosen: an entry, or "New configuration…" with the first one listed, which it
/// is made from when nothing at point runs.
enum Chosen {
    Entry(Entry),
    New(Option<Configuration>),
}

fn run_settings(editor: &Editor) -> RunSettings {
    editor
        .find_plugin_state::<Sessions>()
        .map(|sessions| sessions.settings.clone())
        .unwrap_or_default()
}

/// The project and what was chosen.
fn take_entry(editor: &mut Editor, value: &str) -> Result<(PathBuf, Chosen), String> {
    let listing = editor.plugin_state::<Chooser>().listing.take().ok_or("No such configuration")?;
    let listing = listing.borrow();
    if value == NEW {
        let first = listing.entries.first().map(|entry| entry.configuration.clone());
        return Ok((listing.root.clone(), Chosen::New(first)));
    }
    let index: usize = value.parse().map_err(|_| "No such configuration")?;
    let entry = listing.entries.get(index).cloned().ok_or("No such configuration")?;
    Ok((listing.root.clone(), Chosen::Entry(entry)))
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
        (root, Chosen::Entry(entry)) => edit_configuration(editor, &root, entry.configuration),
        (root, Chosen::New(first)) => new_configuration(editor, &root, first),
    }
}

/// Asks which of the `running` sessions to stop.
pub fn choose_running(editor: &mut Editor, running: Vec<(u64, String)>) -> CommandResult {
    let candidates: Vec<Candidate> = running
        .iter()
        .map(|(id, name)| Candidate { value: id.to_string(), ..Candidate::new(name.clone()) })
        .collect();
    let prompt = Prompt { label: "Stop".into(), require_match: true, ..Prompt::default() };
    editor.read(
        prompt,
        move |_, input| filter_candidates(input, candidates.clone(), false),
        |editor, value| {
            let id = value.parse().map_err(|_| "No such run")?;
            stop_session(editor, id)
        },
    )
}
