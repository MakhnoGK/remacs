//! Going to where the buffer was last changed, then to the changes before, like Emacs
//! `goto-last-change`, read from the undo history.

use remacs_core::{CommandResult, Editor};

pub const GOTO_LAST_CHANGE: &str = "goto-last-change";
pub const GOTO_LAST_CHANGE_REVERSE: &str = "goto-last-change-reverse";

/// How far back the last run went, 0 the latest change.
#[derive(Default)]
struct Visited(usize);

pub(crate) fn goto_last_change(editor: &mut Editor) -> CommandResult {
    let index = match continued(editor) {
        Some(index) => index + 1,
        None => 0,
    };
    go_to_change(editor, index)
}

pub(crate) fn goto_last_change_reverse(editor: &mut Editor) -> CommandResult {
    let index =
        continued(editor).and_then(|index| index.checked_sub(1)).ok_or("No later change")?;
    go_to_change(editor, index)
}

/// Where the last run left off, when it ran just before.
fn continued(editor: &mut Editor) -> Option<usize> {
    let continuing =
        matches!(editor.last_command(), Some(GOTO_LAST_CHANGE | GOTO_LAST_CHANGE_REVERSE));
    continuing.then(|| editor.plugin_state::<Visited>().0)
}

fn go_to_change(editor: &mut Editor, index: usize) -> CommandResult {
    let text = editor.buffer().text();
    let offset = change_places(text.change_offsets(), |offset| {
        let offset = offset.min(text.len());
        (offset, text.line_of(offset))
    })
    .nth(index)
    .ok_or(if index == 0 { "No changes in this buffer" } else { "No earlier change" })?;
    let offset =
        if text.is_char_boundary(offset) { offset } else { text.previous_char_boundary(offset) };
    editor.plugin_state::<Visited>().0 = index;
    let buffer = editor.buffer_mut();
    buffer.set_point(offset);
    buffer.deactivate_mark();
    Ok(())
}

/// The offsets of changes, one per run of changes on the same line, as typing a line is.
fn change_places(
    offsets: impl Iterator<Item = usize>,
    place: impl Fn(usize) -> (usize, usize),
) -> impl Iterator<Item = usize> {
    let mut last_line = None;
    offsets
        .map(place)
        .filter_map(move |(offset, line)| (last_line.replace(line) != Some(line)).then_some(offset))
}
