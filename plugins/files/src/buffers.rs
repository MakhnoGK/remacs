use std::path::Path;

use remacs_core::{
    Buffer, Candidate, CommandResult, Completions, Count, Editor, Prompt, filter_candidates,
};
use remacs_text::Text;

use crate::project::{default_directory, project_root, relative_directory};

const BUFFER_CATEGORY: &str = "buffer";

/// Shows another buffer; a new name creates an empty buffer, as in Emacs.
pub fn switch_to_buffer(editor: &mut Editor) -> CommandResult {
    let prompt = Prompt {
        label: "Buffer".into(),
        category: BUFFER_CATEGORY,
        placeholder: "Switch to buffer…",
        default: editor
            .buffers()
            .get(1)
            .map(|buffer| buffer.name().to_owned()),
        count: Count::Total,
        hints: &[("↵", "Switch"), ("C-x k", "Kill buffer"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(prompt, buffer_candidates, |editor, name| {
        if !editor.switch_to_buffer(&name) {
            let mode = editor.mode_for(None);
            editor.add_buffer(Buffer::new(name, Text::default(), mode));
        }
        Ok(())
    })
}

/// Kills a buffer, asking first if it has unsaved changes; inside a buffer prompt it kills
/// the selected candidate instead.
pub fn kill_buffer(editor: &mut Editor) -> CommandResult {
    if let Some(minibuffer) = editor.minibuffer() {
        if minibuffer.prompt().category != BUFFER_CATEGORY {
            return Err("Command attempted to use minibuffer while in minibuffer".into());
        }
        let name = minibuffer
            .selected_candidate()
            .ok_or("No buffer selected")?
            .value
            .clone();
        if has_unsaved_changes(editor, &name) {
            return Err(format!("Buffer {name} modified; kill it with C-x k").into());
        }
        editor.kill_buffer(&name);
        editor.refresh_minibuffer();
        return Ok(());
    }
    let prompt = Prompt {
        label: "Kill buffer".into(),
        category: BUFFER_CATEGORY,
        default: Some(editor.window_buffer().name().to_owned()),
        require_match: true,
        count: Count::Total,
        hints: &[("↵", "Kill"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(prompt, buffer_candidates, |editor, name| {
        if !has_unsaved_changes(editor, &name) {
            editor.kill_buffer(&name);
            return Ok(());
        }
        editor.ask_y_or_n(
            format!("Buffer {name} modified; kill anyway?"),
            move |editor, yes| {
                if yes {
                    editor.kill_buffer(&name);
                }
                Ok(())
            },
        )
    })
}

/// Buffers in recently-used order, as in Emacs `buffer-list`.
fn buffer_candidates(editor: &Editor, input: &str) -> Completions {
    let root = project_root(&default_directory(editor));
    let current = editor.window_buffer().name();
    let candidates = editor
        .buffers()
        .iter()
        .map(|buffer| Candidate {
            annotation: buffer
                .path()
                .and_then(Path::parent)
                .map(|directory| relative_directory(&root, directory))
                .unwrap_or_default(),
            tag: (buffer.name() == current).then_some("current"),
            detail: Some(buffer.mode()),
            group: Some("Open"),
            modified: buffer.path().is_some() && buffer.is_modified(),
            dim: buffer.name().starts_with('*'),
            ..Candidate::new(buffer.name())
        })
        .collect();
    filter_candidates(input, candidates, false)
}

pub fn has_unsaved_changes(editor: &Editor, name: &str) -> bool {
    editor
        .buffers()
        .iter()
        .any(|buffer| buffer.name() == name && buffer.path().is_some() && buffer.is_modified())
}
