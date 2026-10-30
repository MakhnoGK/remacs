//! Every breakpoint, by project, with keys that enable, visit, edit or delete one.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use remacs_core::{Candidate, Command, CommandResult, Completions, Editor, Keymap, Prompt};
use remacs_files::project_root;

use crate::breakpoints::Breakpoint;
use crate::edit;
use crate::session::{self, Debugger, debugger};

pub const OPEN: &str = "debug-breakpoints";
const ENABLE: &str = "debug-breakpoints-enable";
const VISIT: &str = "debug-breakpoints-visit";
const EDIT: &str = "debug-breakpoints-edit";
const DELETE: &str = "debug-breakpoints-delete";

#[rustfmt::skip]
pub const COMMANDS: &[Command] = &[
    Command::new(OPEN, "List all breakpoints", open),
    Command::new(ENABLE, "Enable the selected breakpoint, or disable it", enable),
    Command::new(VISIT, "Visit the selected breakpoint's line", visit),
    Command::new(EDIT, "Edit the selected breakpoint", edit_selected),
    Command::new(DELETE, "Delete the selected breakpoint", delete),
];

const HINTS: &[(&str, &str)] =
    &[("SPC", "Enable"), ("↵", "Go to"), ("e", "Edit"), ("d", "Delete"), ("C-g", "Close")];

/// The lines breakpoints are on, read when the list opened.
#[derive(Default)]
struct Sources {
    lines: HashMap<(PathBuf, usize), String>,
}

pub fn open(editor: &mut Editor) -> CommandResult {
    let breakpoints = all(editor);
    if breakpoints.is_empty() {
        return Err("No breakpoints; s-<f8> adds one on point's line".into());
    }
    let lines = breakpoints
        .iter()
        .map(|(path, breakpoint)| {
            let text = source_line(editor, path, breakpoint.line).unwrap_or_default();
            ((path.clone(), breakpoint.line), text)
        })
        .collect();
    editor.plugin_state::<Sources>().lines = lines;
    let mut keymap = Keymap::default();
    for (keys, command) in [("SPC", ENABLE), ("RET", VISIT), ("e", EDIT), ("d", DELETE)] {
        keymap.bind(keys, command);
    }
    let prompt = Prompt {
        label: "Breakpoints".into(),
        read_only: true,
        title: Some(("Breakpoints".into(), String::new())),
        wide: true,
        hints: HINTS,
        keymap: Some(keymap),
        ..Prompt::default()
    };
    editor.read(prompt, |editor, _| completions(editor), |_, _| Ok(()))
}

fn all(editor: &mut Editor) -> Vec<(PathBuf, Breakpoint)> {
    debugger(editor)
        .breakpoints
        .files
        .iter()
        .flat_map(|(path, breakpoints)| {
            breakpoints.iter().map(|breakpoint| (path.clone(), breakpoint.clone()))
        })
        .collect()
}

fn completions(editor: &Editor) -> Completions {
    let (Some(debugger), Some(sources)) =
        (editor.find_plugin_state::<Debugger>(), editor.find_plugin_state::<Sources>())
    else {
        return Completions::default();
    };
    let mut candidates: Vec<Candidate> = Vec::new();
    for (path, breakpoints) in &debugger.breakpoints.files {
        let project = path.parent().map(project_root);
        let group = project
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().to_uppercase());
        for breakpoint in breakpoints {
            let source =
                sources.lines.get(&(path.clone(), breakpoint.line)).cloned().unwrap_or_default();
            candidates.push(Candidate {
                value: candidates.len().to_string(),
                label: edit::location(path, breakpoint.line),
                annotation: source,
                detail: Some(describe(breakpoint).into()),
                group: group.clone().map(Into::into),
                dim: !breakpoint.enabled,
                ..Candidate::default()
            });
        }
    }
    let count = candidates.len();
    Completions { subject: Some(format!("{count} in all projects")), ..candidates.into() }
}

/// What it does beside stopping, or why it does not.
fn describe(breakpoint: &Breakpoint) -> String {
    if !breakpoint.enabled {
        return "disabled".into();
    }
    if breakpoint.verified == Some(false) {
        return match &breakpoint.message {
            Some(message) => format!("not verified · {message}"),
            None => "not verified".into(),
        };
    }
    let parts: Vec<String> = [
        breakpoint.condition.as_ref().map(|condition| format!("if {condition}")),
        breakpoint.hit_condition.as_ref().map(|hits| format!("hits {hits}")),
        breakpoint.log_message.as_ref().map(|message| format!("log \"{message}\"")),
    ]
    .into_iter()
    .flatten()
    .collect();
    parts.join(" · ")
}

/// `line` of the file, from its buffer if it is open, else from disk.
fn source_line(editor: &Editor, path: &Path, line: usize) -> Option<String> {
    let text = match editor.buffers().iter().find(|buffer| buffer.path() == Some(path)) {
        Some(buffer) => {
            let range = buffer.text().line_range(line)?;
            buffer.contents(range)
        }
        None => fs::read_to_string(path).ok()?.lines().nth(line)?.to_owned(),
    };
    Some(text.trim().to_owned())
}

/// The selected row's breakpoint.
fn selected(editor: &mut Editor) -> Result<(PathBuf, Breakpoint), &'static str> {
    let index: usize = editor
        .minibuffer()
        .and_then(|minibuffer| minibuffer.selected_candidate())
        .and_then(|candidate| candidate.value.parse().ok())
        .ok_or("No breakpoint")?;
    all(editor).into_iter().nth(index).ok_or("No breakpoint")
}

fn changed(editor: &mut Editor, path: &Path) {
    debugger(editor).breakpoints.save();
    session::breakpoints_changed(editor, path);
    editor.refresh_minibuffer();
}

fn enable(editor: &mut Editor) -> CommandResult {
    let (path, breakpoint) = selected(editor)?;
    if let Some(breakpoint) = debugger(editor).breakpoints.at_mut(&path, breakpoint.line) {
        breakpoint.enabled = !breakpoint.enabled;
    }
    changed(editor, &path);
    Ok(())
}

fn visit(editor: &mut Editor) -> CommandResult {
    let (path, breakpoint) = selected(editor)?;
    editor.cancel_minibuffer();
    editor.visit_file(&path)?;
    editor.set_point_at(breakpoint.line, 0);
    Ok(())
}

fn edit_selected(editor: &mut Editor) -> CommandResult {
    let (path, breakpoint) = selected(editor)?;
    editor.cancel_minibuffer();
    edit::open(editor, &path, breakpoint.line)
}

fn delete(editor: &mut Editor) -> CommandResult {
    let (path, breakpoint) = selected(editor)?;
    debugger(editor).breakpoints.remove(&path, breakpoint.line);
    changed(editor, &path);
    if all(editor).is_empty() {
        editor.cancel_minibuffer();
    }
    Ok(())
}
