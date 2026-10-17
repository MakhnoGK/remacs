//! Built-in plugin: formats the buffer or the region, like Emacs `eglot-format`, and,
//! when turned on, files after each save, like apheleia. It knows nothing of formatters;
//! backends such as prettier or language servers take its requests and deliver the edits.

mod diff;

use std::collections::HashMap;
use std::mem;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use remacs_core::{
    Command, CommandResult, Editor, Embedded, MenuItem, Plugin, Registry, Section, blank_outside,
    visiting_file,
};
use remacs_text::{Snapshot, Text};
use serde::Deserialize;

pub use diff::edits;

pub struct FormatPlugin;

const FORMAT: &str = "format";
const FORMAT_BUFFER: &str = "format-buffer";
const FORMAT_ON_SAVE_MODE: &str = "format-on-save-mode";

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(FORMAT, "Format the region, or the buffer without one", format),
    Command::new(FORMAT_BUFFER, "Format the whole buffer", format_buffer),
    Command::new(FORMAT_ON_SAVE_MODE, "Turn formatting files after each save on or off", toggle_format_on_save),
];

const BINDINGS: &[(&str, &str)] = &[("C-c C-f", FORMAT)];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("Edit", "code", "Format", FORMAT).enabled_when(visiting_file),
    MenuItem::new("Edit", "code", "Format Buffer", FORMAT_BUFFER).enabled_when(visiting_file),
    MenuItem::new("Edit", "code", "Format on Save On or Off", FORMAT_ON_SAVE_MODE),
];

impl Plugin for FormatPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend_from_slice(MENU_ITEMS);
        registry.post_command_hooks.push(Rc::new(format_saved));
        registry.settings_hooks.push(Rc::new(read_settings));
    }
}

/// `format.toml`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
struct FormatSettings {
    /// Whether `format-on-save-mode` starts on.
    on_save: bool,
}

impl Section for FormatSettings {
    const NAME: &str = "format";
}

/// What a backend is asked to format.
#[derive(Clone)]
pub struct Request {
    pub id: u64,
    pub path: PathBuf,
    pub mode: &'static str,
    pub text: Snapshot,
    /// The region; the whole text without one.
    pub range: Option<Range<usize>>,
}

/// Why a request was not formatted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Nothing formats such files or regions; not worth a word after a save.
    Unsupported(String),
    /// The formatter failed, e.g. on a syntax error.
    Failed(String),
}

/// Replacements in offsets of the request's text.
pub type Formatted = Result<Vec<(Range<usize>, String)>, Failure>;

#[derive(Default)]
struct State {
    next_id: u64,
    requests: Vec<Request>,
    /// Asked and not yet answered, by id.
    pending: HashMap<u64, Asked>,
    on_save: bool,
    /// As last read; the session's toggle holds until they change.
    settings: Option<FormatSettings>,
}

struct Asked {
    /// The file and its text when asked: the request's, unless it is of embedded code.
    file: PathBuf,
    basis: Snapshot,
    /// Where the embedded code asked about is, which the edits must stay within.
    code: Option<Embedded>,
    /// Asked after a save, which is made again with the edits.
    after_save: bool,
}

/// How many times the buffer was saved when last looked at.
struct SeenSaves(u64);

fn format(editor: &mut Editor) -> CommandResult {
    in_buffer(editor)?;
    let region = editor.window_buffer().region();
    ask(editor, 0, region, false)
}

fn format_buffer(editor: &mut Editor) -> CommandResult {
    in_buffer(editor)?;
    ask(editor, 0, None, false)
}

fn in_buffer(editor: &Editor) -> CommandResult {
    match editor.minibuffer() {
        Some(_) => Err("Formatting works in buffers only".into()),
        None => Ok(()),
    }
}

/// Settings read again because another file changed keep the mode as toggled.
fn read_settings(editor: &mut Editor) {
    let settings = editor.settings_section::<FormatSettings>();
    let state = editor.plugin_state::<State>();
    if state.settings.as_ref() != Some(&settings) {
        state.on_save = settings.on_save;
        state.settings = Some(settings);
    }
}

fn toggle_format_on_save(editor: &mut Editor) -> CommandResult {
    let state = editor.plugin_state::<State>();
    state.on_save = !state.on_save;
    let message = if state.on_save {
        "Formatting on save enabled"
    } else {
        "Formatting on save disabled"
    };
    editor.set_message(message);
    Ok(())
}

/// Asks to format buffer `index`, its region `range` or all of it. A region within code of
/// another language, like CSS in HTML's `<style>`, is that language's to format, in the
/// document of the code alone.
fn ask(
    editor: &mut Editor,
    index: usize,
    range: Option<Range<usize>>,
    after_save: bool,
) -> CommandResult {
    let buffer = &editor.buffers()[index];
    let file = buffer
        .path()
        .ok_or("The buffer visits no file")?
        .to_path_buf();
    let basis = buffer.text().snapshot();
    let code = range.as_ref().and_then(|range| {
        let embedded = editor.embedded(buffer)?;
        embedded.into_iter().find(|code| code.holds_range(range))
    });
    let (path, mode, text) = match &code {
        Some(code) => {
            let blanked = blank_outside(&basis, 0..basis.len(), &code.ranges);
            let path = editor.embedded_document(&file, code.mode);
            (path, code.mode, Text::from(blanked.as_str()).snapshot())
        }
        None => (file.clone(), buffer.mode(), basis.clone()),
    };
    let state = editor.plugin_state::<State>();
    state.next_id += 1;
    let request = Request {
        id: state.next_id,
        path,
        mode,
        text,
        range,
    };
    let asked = Asked {
        file,
        basis,
        code,
        after_save,
    };
    state.pending.insert(request.id, asked);
    state.requests.push(request);
    Ok(())
}

/// Asks to format the buffers saved since the last command, with formatting on save on.
/// A save is told from undoing back to the saved text by the save count.
fn format_saved(editor: &mut Editor) {
    let on_save = editor
        .find_plugin_state::<State>()
        .is_some_and(|state| state.on_save);
    let mut saved = Vec::new();
    for (index, buffer) in editor.buffers_mut().iter_mut().enumerate() {
        let saves = buffer.text().saves();
        let seen = buffer.local::<SeenSaves>().map_or(0, |seen| seen.0);
        if saves != seen {
            buffer.set_local(SeenSaves(saves));
            if on_save && buffer.path().is_some() {
                saved.push(index);
            }
        }
    }
    for index in saved {
        let _ = ask(editor, index, None, true);
    }
}

/// The requests `accepts` takes on; the rest are left for the backends after it.
pub fn take_requests(editor: &mut Editor, accepts: impl Fn(&Request) -> bool) -> Vec<Request> {
    let state = editor.plugin_state::<State>();
    let (taken, left) = mem::take(&mut state.requests)
        .into_iter()
        .partition(accepts);
    state.requests = left;
    taken
}

/// Makes the edits of request `id` in the buffer visiting its file, as one undo step,
/// unless the text changed since it was asked for; after a save, saves again.
pub fn deliver(editor: &mut Editor, id: u64, formatted: Formatted) {
    let Some(Asked {
        file,
        basis,
        code,
        after_save,
    }) = editor.plugin_state::<State>().pending.remove(&id)
    else {
        return;
    };
    let edits = match formatted {
        Ok(edits) => edits,
        Err(Failure::Unsupported(_)) if after_save => return,
        Err(Failure::Unsupported(message) | Failure::Failed(message)) => {
            return editor.set_error(message);
        }
    };
    if let Some(code) = &code
        && !edits.iter().all(|(edit, _)| code.holds_range(edit))
    {
        return editor.set_error("Not formatted: the edits reach outside the embedded code");
    }
    let Some(buffer) = editor
        .buffers_mut()
        .iter_mut()
        .find(|buffer| buffer.path() == Some(&file))
    else {
        return;
    };
    if buffer.text().snapshot().edit_from(&basis).is_some() {
        // Typing after a save is left alone; the next save formats it.
        if !after_save {
            editor.set_message("Not formatted: the buffer changed meanwhile");
        }
        return;
    }
    if edits.is_empty() {
        return;
    }
    let point = point_after(buffer.point(), &edits);
    buffer.add_undo_boundary();
    buffer.apply_edits(edits);
    buffer.set_point(point);
    if !after_save {
        return;
    }
    match buffer.save() {
        Ok(()) => {
            let saves = buffer.text().saves();
            buffer.set_local(SeenSaves(saves));
        }
        Err(error) => editor.set_error(format!("Formatted, not saved: {error}")),
    }
}

/// Where point ends up after `edits`: on its character, so space inserted before it, at
/// point, keeps it there.
fn point_after(point: usize, edits: &[(Range<usize>, String)]) -> usize {
    let shift: isize = edits
        .iter()
        .map(|(range, text)| {
            if range.end <= point {
                text.len() as isize - range.len() as isize
            } else if range.start < point {
                (point - range.start).min(text.len()) as isize - (point - range.start) as isize
            } else {
                0
            }
        })
        .sum();
    point.saturating_add_signed(shift)
}

#[cfg(test)]
mod tests;
