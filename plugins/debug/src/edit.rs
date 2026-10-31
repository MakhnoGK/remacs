//! Editing a breakpoint: what it stops on (a condition or a hit count), or the message it
//! logs instead of stopping.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_core::{
    Command, CommandResult, Completions, Editor, Keymap, Prompt, PromptChoice, quote,
};
use remacs_files::project_root;

use crate::breakpoints::Breakpoint;
use crate::list;
use crate::marks::point_line;
use crate::session::{self, debugger};

pub const EDIT: &str = "debug-edit-breakpoint";
const REMOVE: &str = "debug-edit-breakpoint-remove";
const OPEN_LIST: &str = "debug-edit-breakpoint-list";

#[rustfmt::skip]
pub const COMMANDS: &[Command] = &[
    Command::new(EDIT, "Edit the breakpoint on point's line: its condition, hit count or log message", edit),
    Command::new(REMOVE, "Remove the breakpoint being edited", remove),
    Command::new(OPEN_LIST, "List all breakpoints instead", open_list),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Setting {
    Condition,
    LogMessage,
    HitCount,
}

const SETTINGS: [Setting; 3] = [Setting::Condition, Setting::LogMessage, Setting::HitCount];

impl Setting {
    fn label(self) -> &'static str {
        match self {
            Setting::Condition => "Condition",
            Setting::LogMessage => "Log message",
            Setting::HitCount => "Hit count",
        }
    }

    fn explanation(self) -> &'static str {
        match self {
            Setting::Condition => "Stops only when the expression is true",
            Setting::LogMessage => {
                "Logs the message instead of stopping; {expressions} are filled in"
            }
            Setting::HitCount => "Stops only once hit so many times, e.g. 3 or >= 3",
        }
    }

    fn of(self, breakpoint: &mut Breakpoint) -> &mut Option<String> {
        match self {
            Setting::Condition => &mut breakpoint.condition,
            Setting::LogMessage => &mut breakpoint.log_message,
            Setting::HitCount => &mut breakpoint.hit_condition,
        }
    }
}

const HINTS: &[(&str, &str)] =
    &[("↵", "Save"), ("M-n", "Type"), ("C-c C-k", "Remove"), ("⌘⇧F8", "All"), ("C-g", "Cancel")];

/// The breakpoint being edited.
#[derive(Default)]
struct Editing {
    at: Option<(PathBuf, usize)>,
}

/// Edits the breakpoint on point's line; where there is none, lists all breakpoints, as
/// IntelliJ does.
pub fn edit(editor: &mut Editor) -> CommandResult {
    match point_line(editor) {
        Ok((path, line)) if debugger(editor).breakpoints.at(&path, line).is_some() => {
            open(editor, &path, line)
        }
        _ => list::open(editor),
    }
}

pub fn open(editor: &mut Editor, path: &Path, line: usize) -> CommandResult {
    let mut breakpoint = debugger(editor)
        .breakpoints
        .at(path, line)
        .cloned()
        .unwrap_or_else(|| Breakpoint::at(line));
    // The one set, the log message first since it changes what the breakpoint does.
    let shown = [Setting::LogMessage, Setting::Condition, Setting::HitCount]
        .into_iter()
        .find(|setting| setting.of(&mut breakpoint).is_some())
        .unwrap_or(Setting::Condition);
    let initial_input = shown.of(&mut breakpoint).clone().unwrap_or_default();
    let index = SETTINGS.iter().position(|setting| *setting == shown).unwrap_or(0);
    let selected = Rc::new(Cell::new(index));
    let mut keymap = Keymap::default();
    keymap.bind("C-c C-k", REMOVE);
    keymap.bind("s-S-<f8>", OPEN_LIST);
    editor.plugin_state::<Editing>().at = Some((path.to_path_buf(), line));
    let prompt = Prompt {
        label: quote(&location(path, line)),
        placeholder: "Empty for none",
        initial_input,
        hints: HINTS,
        keymap: Some(keymap),
        chips_first: true,
        choices: vec![PromptChoice {
            label: "Type",
            key: "M-n",
            values: SETTINGS.iter().map(|setting| setting.label().to_owned()).collect(),
            selected: Rc::clone(&selected),
            segmented: true,
        }],
        ..Prompt::default()
    };
    let path = path.to_path_buf();
    let shown_selection = Rc::clone(&selected);
    editor.read(
        prompt,
        move |_, _| Completions {
            note: Some(SETTINGS[shown_selection.get()].explanation().to_owned()),
            ..Completions::default()
        },
        move |editor, input| {
            let input = input.trim();
            let breakpoints = &mut debugger(editor).breakpoints;
            let mut breakpoint =
                breakpoints.at(&path, line).cloned().unwrap_or_else(|| Breakpoint::at(line));
            *SETTINGS[selected.get()].of(&mut breakpoint) =
                (!input.is_empty()).then(|| input.to_owned());
            breakpoint.enabled = true;
            breakpoints.add(&path, breakpoint);
            breakpoints.save();
            session::breakpoints_changed(editor, &path);
            Ok(())
        },
    )
}

/// `src/main.rs:12`, relative to the file's project.
pub fn location(path: &Path, line: usize) -> String {
    let root = path.parent().map(project_root);
    let relative = root.as_deref().and_then(|root| path.strip_prefix(root).ok()).unwrap_or(path);
    format!("{}:{}", relative.display(), line + 1)
}

fn remove(editor: &mut Editor) -> CommandResult {
    let (path, line) = editor.plugin_state::<Editing>().at.take().ok_or("No breakpoint")?;
    editor.cancel_minibuffer();
    let breakpoints = &mut debugger(editor).breakpoints;
    breakpoints.remove(&path, line);
    breakpoints.save();
    session::breakpoints_changed(editor, &path);
    Ok(())
}

fn open_list(editor: &mut Editor) -> CommandResult {
    editor.cancel_minibuffer();
    list::open(editor)
}
