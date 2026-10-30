//! The Tools panel: every tool, what is installed of it and how its download goes, with
//! keys that update, reinstall or cancel one.

use std::borrow::Cow;

use remacs_core::{Candidate, Command, CommandResult, Completions, Editor, Keymap, Prompt, quote};
use remacs_files::abbreviate_home;

use crate::catalog::ToolKind;
use crate::state::{self, Entry, Progress, Tools, tools};

pub const OPEN: &str = "tools";
pub const CATEGORY: &str = "tool";
const ACT: &str = "tools-act";
const UPDATE: &str = "tools-update";
const INSTALL: &str = "tools-install";
const REINSTALL: &str = "tools-reinstall";
const CANCEL: &str = "tools-cancel";
const SETTINGS: &str = "tools-open-settings";

#[rustfmt::skip]
pub const COMMANDS: &[Command] = &[
    Command::new(OPEN, "Show the tools remacs downloads and how their downloads go", open),
    Command::new(ACT, "Do what the selected tool needs: install, update or cancel it", act),
    Command::new(UPDATE, "Install the latest release of the selected tool", update),
    Command::new(INSTALL, "Install the selected tool, or try again after it failed", install),
    Command::new(REINSTALL, "Download the selected tool again", reinstall),
    Command::new(CANCEL, "Stop installing the selected tool", cancel),
    Command::new(SETTINGS, "Visit the tools' settings", open_settings),
];

const KEYS: &[(&str, &str)] = &[
    ("RET", ACT),
    ("u", UPDATE),
    ("r", INSTALL),
    ("R", REINSTALL),
    ("k", CANCEL),
    ("o", SETTINGS),
];

const HINTS: &[(&str, &str)] = &[
    ("↵", "Act"),
    ("u", "Update"),
    ("r", "Retry"),
    ("R", "Reinstall"),
    ("k", "Cancel"),
    ("o", "Settings"),
    ("C-g", "Close"),
];

/// What the selected row's key does now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Install,
    Update,
    Reinstall,
    Cancel,
    OpenSettings,
}

impl Action {
    fn key(self) -> &'static str {
        match self {
            Action::Install => "r",
            Action::Update => "u",
            Action::Reinstall => "R",
            Action::Cancel => "k",
            Action::OpenSettings => "o",
        }
    }
}

fn action(tools: &Tools, entry: &Entry) -> Action {
    match &entry.progress {
        _ if tools.own_program(entry.tool.name).is_some() => Action::OpenSettings,
        Progress::Downloading { .. } | Progress::Installing { .. } => Action::Cancel,
        Progress::Failed(_) => Action::Install,
        Progress::Idle if entry.installed.is_none() => Action::Install,
        Progress::Idle if entry.update.is_some() => Action::Update,
        Progress::Idle => Action::Reinstall,
    }
}

pub fn is_open(editor: &Editor) -> bool {
    editor.minibuffer().is_some_and(|minibuffer| minibuffer.prompt().category == CATEGORY)
}

pub fn open(editor: &mut Editor) -> CommandResult {
    state::check_updates(editor);
    let dir = tools(editor).dir.as_deref().map(abbreviate_home).unwrap_or_default();
    let mut keymap = Keymap::default();
    for &(keys, command) in KEYS {
        keymap.bind(keys, command);
    }
    let prompt = Prompt {
        label: "Tools".into(),
        category: CATEGORY,
        read_only: true,
        title: Some(("Tools".into(), quote(&dir))),
        wide: true,
        hints: HINTS,
        keymap: Some(keymap),
        ..Prompt::default()
    };
    editor.read(prompt, |editor, _| completions(editor), |_, _| Ok(()))
}

fn completions(editor: &Editor) -> Completions {
    let Some(tools) = editor.find_plugin_state::<Tools>() else {
        return Completions::default();
    };
    let candidates: Vec<Candidate> =
        tools.entries.iter().map(|entry| candidate(tools, entry)).collect();
    Completions {
        subject: Some("Debug adapters and language servers remacs downloads".into()),
        ..candidates.into()
    }
}

fn candidate(tools: &Tools, entry: &Entry) -> Candidate {
    let name = entry.tool.name;
    let own = tools.own_program(name);
    let annotation = match (&own, &entry.progress, &entry.installed) {
        (Some(own), _, _) => abbreviate_home(own),
        (_, Progress::Downloading { version: Some(version), .. }, _)
        | (_, Progress::Installing { version }, _)
        | (_, _, Some(version)) => format!("{version} · {}", entry.tool.origin()),
        _ => entry.tool.origin().into(),
    };
    Candidate {
        value: name.into(),
        label: name.into(),
        annotation,
        detail: Some(Cow::Owned(describe(tools, entry, own.is_some()))),
        keys: vec![action(tools, entry).key().into()],
        group: Some(entry.tool.kind.group().into()),
        ..Candidate::default()
    }
}

fn describe(tools: &Tools, entry: &Entry, own: bool) -> String {
    match &entry.progress {
        _ if own => "Set in tools.toml".into(),
        Progress::Downloading { percent: None, .. } => "Looking up the release…".into(),
        Progress::Downloading { percent: Some(percent), .. } => format!("Downloading {percent}%"),
        Progress::Installing { .. } => "Installing…".into(),
        Progress::Failed(error) => format!("Failed: {error}"),
        Progress::Idle => match (&entry.installed, &entry.update) {
            (Some(_), Some(update)) => format!("Update available · {update}"),
            (Some(_), None) => "Installed".into(),
            (None, _) if !tools.can_install(entry.tool) => {
                format!("Needs {} on PATH", tools.needs(entry.tool).join(", "))
            }
            (None, _) if entry.tool.kind != ToolKind::LanguageServer => {
                "Downloaded on first use".into()
            }
            (None, _) => "Not installed".into(),
        },
    }
}

/// The selected row's tool.
fn selected(editor: &Editor) -> Result<&'static str, &'static str> {
    let value = editor
        .minibuffer()
        .and_then(|minibuffer| minibuffer.selected_candidate())
        .map(|candidate| candidate.value.as_str())
        .ok_or("No tool")?;
    editor
        .find_plugin_state::<Tools>()
        .and_then(|tools| tools.entry(value))
        .map(|entry| entry.tool.name)
        .ok_or("No tool")
}

fn act(editor: &mut Editor) -> CommandResult {
    let name = selected(editor)?;
    let tools = tools(editor);
    let entry = tools.entry(name).ok_or("No tool")?;
    match action(tools, entry) {
        Action::Install => install(editor),
        Action::Update => update(editor),
        Action::Cancel => cancel(editor),
        Action::OpenSettings => open_settings(editor),
        Action::Reinstall => {
            let version = entry.installed.clone().unwrap_or_default();
            editor.set_message(format!("{name} {version} is installed; R downloads it again"));
            Ok(())
        }
    }
}

fn update(editor: &mut Editor) -> CommandResult {
    let name = selected(editor)?;
    if tools(editor).own_program(name).is_some() {
        return Err(format!("{name} is set in tools.toml").into());
    }
    let latest = tools(editor).entry(name).and_then(|entry| entry.update.clone());
    state::start_install(editor, name, latest, false);
    editor.refresh_minibuffer();
    Ok(())
}

fn install(editor: &mut Editor) -> CommandResult {
    let name = selected(editor)?;
    state::start_install(editor, name, None, false);
    editor.refresh_minibuffer();
    Ok(())
}

fn reinstall(editor: &mut Editor) -> CommandResult {
    let name = selected(editor)?;
    let version = tools(editor).entry(name).and_then(|entry| entry.installed.clone());
    state::start_install(editor, name, version, true);
    editor.refresh_minibuffer();
    Ok(())
}

fn cancel(editor: &mut Editor) -> CommandResult {
    let name = selected(editor)?;
    state::cancel(editor, name);
    Ok(())
}

fn open_settings(editor: &mut Editor) -> CommandResult {
    let path = tools(editor).settings_file.clone().ok_or("No settings directory")?;
    editor.cancel_minibuffer();
    editor.visit_file(&path)?;
    Ok(())
}
