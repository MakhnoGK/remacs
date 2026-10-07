//! What is installed of each tool, what is being installed, and who waits for it.

use std::path::PathBuf;

use remacs_core::Editor;
use remacs_files::expand_home;

use crate::catalog::{TOOLS, Tool, version_key};
use crate::install::{Event, Job, JobKind, Place, installed_versions};
use crate::settings::ToolsSettings;

pub const GITHUB_API: &str = "https://api.github.com";

/// Called once a tool's program is there, or could not be got.
pub type Waiter = Box<dyn FnOnce(&mut Editor, Result<PathBuf, String>)>;

pub struct Tools {
    pub settings: ToolsSettings,
    /// Where tools are installed; none, nothing is.
    pub dir: Option<PathBuf>,
    /// `tools.toml`.
    pub settings_file: Option<PathBuf>,
    pub api: String,
    /// One per tool of the catalog, in its order.
    pub entries: Vec<Entry>,
    waiters: Vec<(&'static str, Waiter)>,
}

pub struct Entry {
    pub tool: &'static Tool,
    pub installed: Option<String>,
    /// A newer release than the one installed.
    pub update: Option<String>,
    pub progress: Progress,
    pub job: Option<Job>,
    /// Looked for updates since remacs started.
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    Idle,
    /// Looking up the release while `percent` is none.
    Downloading {
        version: Option<String>,
        percent: Option<u8>,
    },
    Unpacking {
        version: String,
    },
    Failed(String),
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            settings: ToolsSettings::default(),
            dir: None,
            settings_file: None,
            api: GITHUB_API.to_owned(),
            entries: TOOLS
                .iter()
                .map(|&tool| Entry {
                    tool,
                    installed: None,
                    update: None,
                    progress: Progress::Idle,
                    job: None,
                    checked: false,
                })
                .collect(),
            waiters: Vec::new(),
        }
    }
}

impl Tools {
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.tool.name == name)
    }

    fn entry_mut(&mut self, name: &str) -> Option<&mut Entry> {
        self.entries
            .iter_mut()
            .find(|entry| entry.tool.name == name)
    }

    /// The version `tools.toml` asks for, if it does.
    pub fn pinned(&self, name: &str) -> Option<&str> {
        self.settings.of(name)?.version.as_deref()
    }

    /// A program of one's own from `tools.toml`, used instead of a download.
    pub fn own_program(&self, name: &str) -> Option<PathBuf> {
        let path = self.settings.of(name)?.path.as_deref()?;
        Some(expand_home(path))
    }

    pub fn program(&self, name: &str) -> Option<PathBuf> {
        if let Some(own) = self.own_program(name) {
            return Some(own);
        }
        let entry = self.entry(name)?;
        let version = entry.installed.as_deref()?;
        Some(
            self.dir
                .as_ref()?
                .join(name)
                .join(version)
                .join(entry.tool.program),
        )
    }

    /// Finds what is installed again, e.g. after `tools.toml` pinned another version.
    pub fn scan(&mut self) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        for index in 0..self.entries.len() {
            let tool = self.entries[index].tool;
            let versions = installed_versions(tool, &dir);
            let installed = match self.pinned(tool.name) {
                Some(pinned) => versions.into_iter().find(|version| version == pinned),
                None => versions
                    .into_iter()
                    .max_by_key(|version| version_key(version)),
            };
            let entry = &mut self.entries[index];
            entry.update = entry
                .update
                .take()
                .filter(|update| is_newer(update, installed.as_deref()));
            entry.installed = installed;
        }
    }

    fn place(&self) -> Option<Place> {
        Some(Place {
            api: self.api.clone(),
            dir: self.dir.clone()?,
        })
    }
}

pub fn tools(editor: &mut Editor) -> &mut Tools {
    editor.plugin_state()
}

pub fn keep_in(editor: &mut Editor, dir: PathBuf, settings_file: PathBuf) {
    let tools = tools(editor);
    tools.dir = Some(dir);
    tools.settings_file = Some(settings_file);
    tools.scan();
}

/// Installs `version` of the tool, else the release `tools.toml` pins, else the latest;
/// the one installed is kept unless `force`. Does nothing while it is already being
/// installed.
pub fn start_install(editor: &mut Editor, name: &str, version: Option<String>, force: bool) {
    let notifier = editor.notifier();
    let tools = tools(editor);
    let Some(place) = tools.place() else {
        fail(editor, name, "nowhere to install tools".into());
        return;
    };
    let version = version.or_else(|| tools.pinned(name).map(str::to_owned));
    let Some(entry) = tools.entry_mut(name) else {
        return;
    };
    if entry
        .job
        .as_ref()
        .is_some_and(|job| job.kind == JobKind::Install)
    {
        return;
    }
    entry.progress = Progress::Downloading {
        version: version.clone(),
        percent: None,
    };
    entry.job = Some(Job::install(entry.tool, version, force, place, notifier));
}

/// Looks up the latest release of each installed tool once, unless one is pinned or of
/// one's own.
pub fn check_updates(editor: &mut Editor) {
    let notifier = editor.notifier();
    let tools = tools(editor);
    if !tools.settings.check_updates {
        return;
    }
    let Some(place) = tools.place() else {
        return;
    };
    let names: Vec<&'static str> = tools
        .entries
        .iter()
        .filter(|entry| !entry.checked && entry.job.is_none() && entry.installed.is_some())
        .map(|entry| entry.tool.name)
        .filter(|name| tools.pinned(name).is_none() && tools.own_program(name).is_none())
        .collect();
    for name in names {
        let entry = tools.entry_mut(name).expect("listed");
        entry.checked = true;
        entry.job = Some(Job::check(entry.tool, place.clone(), notifier.clone()));
    }
}

pub fn cancel(editor: &mut Editor, name: &str) {
    if let Some(job) = tools(editor)
        .entry(name)
        .and_then(|entry| entry.job.as_ref())
    {
        job.cancel();
    }
}

/// Calls `then` with the tool's program once it is there, installing it first if it is
/// not.
pub fn ensure(editor: &mut Editor, name: &str, then: Waiter) {
    let tools = tools(editor);
    let Some(tool) = tools.entry(name).map(|entry| entry.tool) else {
        then(editor, Err(format!("no tool is called {name}")));
        return;
    };
    if let Some(program) = tools.program(name) {
        then(editor, Ok(program));
        return;
    }
    tools.waiters.push((tool.name, then));
    start_install(editor, tool.name, None, false);
}

fn fail(editor: &mut Editor, name: &str, error: String) {
    if let Some(entry) = tools(editor).entry_mut(name) {
        entry.progress = Progress::Failed(error.clone());
    }
    finish_waiters(editor, name, Err(error));
}

fn finish_waiters(editor: &mut Editor, name: &str, result: Result<PathBuf, String>) {
    let tools = tools(editor);
    let (done, waiting) = std::mem::take(&mut tools.waiters)
        .into_iter()
        .partition(|(waiting_for, _)| *waiting_for == name);
    tools.waiters = waiting;
    for (_, then) in done {
        then(editor, result.clone());
    }
}

/// Takes what the jobs found out; true when anything changed.
pub fn pick_up(editor: &mut Editor) -> bool {
    let mut changed = false;
    for index in 0..tools(editor).entries.len() {
        loop {
            let entry = &mut tools(editor).entries[index];
            let Some(job) = &entry.job else {
                break;
            };
            let kind = job.kind;
            let Some(event) = job.try_event() else {
                break;
            };
            changed = true;
            let name = entry.tool.name;
            match event {
                Event::Found { version } => {
                    entry.progress = Progress::Downloading {
                        version: Some(version),
                        percent: Some(0),
                    }
                }
                Event::Progress(percent) => {
                    if let Progress::Downloading { percent: shown, .. } = &mut entry.progress {
                        *shown = Some(percent);
                    }
                }
                Event::Unpacking => {
                    if let Progress::Downloading {
                        version: Some(version),
                        ..
                    } = &entry.progress
                    {
                        entry.progress = Progress::Unpacking {
                            version: version.clone(),
                        };
                    }
                }
                Event::Installed { version } => {
                    entry.job = None;
                    entry.progress = Progress::Idle;
                    entry.installed = Some(version);
                    tools(editor).scan();
                    let program = tools(editor).program(name).ok_or("not installed");
                    finish_waiters(editor, name, program.map_err(str::to_owned));
                }
                Event::Latest { version } => {
                    entry.job = None;
                    let newer = is_newer(&version, entry.installed.as_deref());
                    entry.update = newer.then_some(version);
                }
                Event::Cancelled => {
                    entry.job = None;
                    entry.progress = Progress::Idle;
                    let error = format!("installing {name} was cancelled");
                    finish_waiters(editor, name, Err(error));
                }
                Event::Failed(error) => {
                    entry.job = None;
                    if kind == JobKind::Install {
                        fail(editor, name, error);
                    }
                }
            }
        }
    }
    changed
}

/// Newer than the version installed, if one is.
fn is_newer(version: &str, installed: Option<&str>) -> bool {
    installed.is_some_and(|installed| version_key(version) > version_key(installed))
}
