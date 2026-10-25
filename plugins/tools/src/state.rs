//! What is installed of each tool, what is being installed, and who waits for it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use remacs_core::Editor;
use remacs_files::expand_home;

use crate::catalog::{DEBUG_ADAPTERS, Source, Tool, ToolKind, version_key};
use crate::find_on_path;
use crate::install::{Event, Job, JobKind, Place, installed_versions, program_path, run_env};
use crate::installers::{self, Installer};
use crate::settings::ToolsSettings;

pub const GITHUB_API: &str = "https://api.github.com";

/// Called once a tool's program is there, or could not be got.
pub type Waiter = Box<dyn FnOnce(&mut Editor, Result<PathBuf, String>)>;

/// An installed tool's program and the environment it runs with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub path: PathBuf,
    pub env: Vec<(String, String)>,
}

pub struct Tools {
    pub settings: ToolsSettings,
    /// Where tools are installed; none, nothing is.
    pub dir: Option<PathBuf>,
    /// `tools.toml`.
    pub settings_file: Option<PathBuf>,
    pub api: String,
    /// The built-in ones with `tools.toml`'s over them.
    pub installers: BTreeMap<String, Installer>,
    /// The debug adapters, then the tools the plugins provide.
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
    /// Unpacking a download, or npm installing a package.
    Installing {
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
            installers: installers::built_in(),
            entries: DEBUG_ADAPTERS
                .iter()
                .map(|&tool| Entry::new(tool))
                .collect(),
            waiters: Vec::new(),
        }
    }
}

impl Entry {
    fn new(tool: &'static Tool) -> Self {
        Self {
            tool,
            installed: None,
            update: None,
            progress: Progress::Idle,
            job: None,
            checked: false,
        }
    }
}

impl Tools {
    /// Replaces the tools of `kind` with `tools`; one of the same name keeps its job.
    pub fn provide(&mut self, kind: ToolKind, tools: &[&'static Tool]) {
        let (mut old, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.entries)
            .into_iter()
            .partition(|entry| entry.tool.kind == kind);
        self.entries = kept;
        for &tool in tools {
            let entry = match old.iter().position(|entry| entry.tool.name == tool.name) {
                Some(index) => Entry {
                    tool,
                    ..old.swap_remove(index)
                },
                None => Entry::new(tool),
            };
            self.entries.push(entry);
        }
        self.scan();
    }

    fn installer(&self, tool: &Tool) -> Option<&Installer> {
        match tool.source {
            Source::GitHub { .. } => None,
            Source::Package { installer, .. } => self.installers.get(installer),
        }
    }

    /// Takes `tools.toml`, its installers over the built-in ones.
    pub fn set_settings(&mut self, settings: ToolsSettings) {
        let mut installers = installers::built_in();
        installers.extend(settings.installers.clone());
        self.installers = installers;
        self.settings = settings;
        self.scan();
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.tool.name == name)
    }

    fn entry_mut(&mut self, name: &str) -> Option<&mut Entry> {
        self.entries
            .iter_mut()
            .find(|entry| entry.tool.name == name)
    }

    /// The version `tools.toml` asks for, else the one the tool must be, if any.
    pub fn pinned(&self, name: &str) -> Option<&str> {
        self.settings
            .of(name)
            .and_then(|settings| settings.version.as_deref())
            .or_else(|| self.entry(name)?.tool.version)
    }

    /// A program of one's own from `tools.toml`, used instead of a download.
    pub fn own_program(&self, name: &str) -> Option<PathBuf> {
        let path = self.settings.of(name)?.path.as_deref()?;
        Some(expand_home(path))
    }

    pub fn program(&self, name: &str) -> Option<PathBuf> {
        self.installed_program(name).map(|program| program.path)
    }

    /// The tool's program, of one's own or installed, with its environment.
    pub fn installed_program(&self, name: &str) -> Option<Program> {
        if let Some(path) = self.own_program(name) {
            return Some(Program {
                path,
                env: Vec::new(),
            });
        }
        let entry = self.entry(name)?;
        let version = entry.installed.as_deref()?;
        let dir = self.dir.as_ref()?;
        let installer = self.installer(entry.tool);
        let path = dir.join(program_path(entry.tool, version, installer));
        let env = installer.map_or_else(Vec::new, |installer| {
            run_env(
                entry.tool,
                &dir.join(entry.tool.home()).join(version),
                installer,
            )
        });
        Some(Program { path, env })
    }

    /// Whether the tool can be installed here: its installer is known and what it needs
    /// is on `PATH`.
    pub fn can_install(&self, tool: &Tool) -> bool {
        self.dir.is_some()
            && tool.builds_here()
            && tool
                .needs
                .iter()
                .all(|program| find_on_path(program).is_some())
            && match tool.source {
                Source::GitHub { .. } => true,
                Source::Package { .. } => self.installer(tool).is_some_and(Installer::can_run),
            }
    }

    /// What the tool and its installer need, for saying why it cannot be installed.
    pub fn needs(&self, tool: &Tool) -> Vec<String> {
        let installer = self.installer(tool).map(|installer| &installer.needs);
        let itself = (!tool.builds_here()).then(|| tool.name.to_owned());
        itself
            .into_iter()
            .chain(tool.needs.iter().map(|program| (*program).to_owned()))
            .chain(installer.into_iter().flatten().cloned())
            .collect()
    }

    /// Finds what is installed again, e.g. after `tools.toml` pinned another version.
    pub fn scan(&mut self) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        for index in 0..self.entries.len() {
            let tool = self.entries[index].tool;
            let versions = installed_versions(tool, &dir, self.installer(tool));
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
            installers: self.installers.clone(),
        })
    }

    /// Whether a tool installed with `tool`, of the same npm package, is being installed.
    fn sibling_installing(&self, tool: &Tool) -> bool {
        self.entries.iter().any(|entry| {
            entry.tool.home() == tool.home()
                && entry
                    .job
                    .as_ref()
                    .is_some_and(|job| job.kind == JobKind::Install)
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
/// the one installed is kept unless `force`. Does nothing while it, or another tool of
/// its npm package, is already being installed.
pub fn start_install(editor: &mut Editor, name: &str, version: Option<String>, force: bool) {
    let notifier = editor.notifier();
    let tools = tools(editor);
    let Some(place) = tools.place() else {
        fail(editor, name, "nowhere to install tools".into());
        return;
    };
    let version = version.or_else(|| tools.pinned(name).map(str::to_owned));
    let Some(tool) = tools.entry(name).map(|entry| entry.tool) else {
        return;
    };
    if !tool.builds_here() {
        fail(
            editor,
            name,
            format!("no build for this system; install {name} on PATH"),
        );
        return;
    }
    if tools.sibling_installing(tool) {
        return;
    }
    let entry = tools.entry_mut(name).expect("found");
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
    finish_waiters(editor, name, |_, _| Err(error.clone()));
}

/// Answers those waiting for `name` and for the other tools of its npm package.
fn finish_waiters(
    editor: &mut Editor,
    name: &str,
    result: impl Fn(&Tools, &str) -> Result<PathBuf, String>,
) {
    let tools = tools(editor);
    let Some(home) = tools.entry(name).map(|entry| entry.tool.home()) else {
        return;
    };
    let (done, waiting): (Vec<_>, _) =
        std::mem::take(&mut tools.waiters)
            .into_iter()
            .partition(|(waiting_for, _)| {
                tools
                    .entry(waiting_for)
                    .is_some_and(|entry| entry.tool.home() == home)
            });
    tools.waiters = waiting;
    let answered: Vec<_> = done
        .into_iter()
        .map(|(waiting_for, then)| (result(tools, waiting_for), then))
        .collect();
    for (result, then) in answered {
        then(editor, result);
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
                Event::Installing => {
                    if let Progress::Downloading {
                        version: Some(version),
                        ..
                    } = &entry.progress
                    {
                        entry.progress = Progress::Installing {
                            version: version.clone(),
                        };
                    }
                }
                Event::Installed { version } => {
                    entry.job = None;
                    entry.progress = Progress::Idle;
                    entry.installed = Some(version.clone());
                    tools(editor).scan();
                    finish_waiters(editor, name, |tools, waiting_for| {
                        tools
                            .program(waiting_for)
                            .ok_or_else(|| format!("{name} {version} has no {waiting_for}"))
                    });
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
                    finish_waiters(editor, name, |_, _| Err(error.clone()));
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
