//! Built-in plugin: formats the buffer or the region, like Emacs `eglot-format`. It knows
//! nothing of formatters; backends such as prettier or language servers take its
//! requests and deliver the edits.

mod diff;

use std::collections::HashMap;
use std::mem;
use std::ops::Range;
use std::path::PathBuf;

use remacs_core::{Command, CommandResult, Editor, Plugin, Registry};
use remacs_text::Snapshot;

pub use diff::edits;

pub struct FormatPlugin;

const FORMAT: &str = "format";
const FORMAT_BUFFER: &str = "format-buffer";

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(FORMAT, "Format the region, or the buffer without one", format),
    Command::new(FORMAT_BUFFER, "Format the whole buffer", format_buffer),
];

const BINDINGS: &[(&str, &str)] = &[("C-c C-f", FORMAT)];

impl Plugin for FormatPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
    }
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

/// Replacements in offsets of the request's text, or why it could not be formatted.
pub type Formatted = Result<Vec<(Range<usize>, String)>, String>;

#[derive(Default)]
struct State {
    next_id: u64,
    requests: Vec<Request>,
    /// Asked and not yet answered, by id.
    pending: HashMap<u64, Request>,
}

fn format(editor: &mut Editor) -> CommandResult {
    let region = editor.window_buffer().region();
    request(editor, region)
}

fn format_buffer(editor: &mut Editor) -> CommandResult {
    request(editor, None)
}

fn request(editor: &mut Editor, range: Option<Range<usize>>) -> CommandResult {
    if editor.minibuffer().is_some() {
        return Err("Formatting works in buffers only".into());
    }
    let buffer = editor.window_buffer();
    let path = buffer
        .path()
        .ok_or("The buffer visits no file")?
        .to_path_buf();
    let (mode, text) = (buffer.mode(), buffer.text().snapshot());
    let state = editor.plugin_state::<State>();
    state.next_id += 1;
    let request = Request {
        id: state.next_id,
        path,
        mode,
        text,
        range,
    };
    state.pending.insert(request.id, request.clone());
    state.requests.push(request);
    Ok(())
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
/// unless the text changed since it was asked for.
pub fn deliver(editor: &mut Editor, id: u64, formatted: Formatted) {
    let Some(request) = editor.plugin_state::<State>().pending.remove(&id) else {
        return;
    };
    let edits = match formatted {
        Ok(edits) => edits,
        Err(message) => return editor.set_message(message),
    };
    let Some(buffer) = editor
        .buffers_mut()
        .iter_mut()
        .find(|buffer| buffer.path() == Some(&request.path))
    else {
        return;
    };
    if buffer.text().snapshot().edit_from(&request.text).is_some() {
        return editor.set_message("Not formatted: the buffer changed meanwhile");
    }
    if !edits.is_empty() {
        let point = point_after(buffer.point(), &edits);
        buffer.add_undo_boundary();
        buffer.apply_edits(edits);
        buffer.set_point(point);
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
