use std::mem;
use std::ops::Range;

use remacs_core::{Buffer, CommandResult, DocBlock, Documentation, Editor, Face, Popup, Span};

pub(crate) const SHOW: &str = "show-documentation";
pub(crate) const HIDE: &str = "hide-documentation";
const HINTS: &[(&str, &str)] = &[("M-.", "Go to definition"), ("C-h .", "Full documentation")];
const FULL_HINTS: &[(&str, &str)] = &[("M-.", "Go to definition")];

/// What the plugin asks of the window buffer's backend: documentation of the symbol at
/// `offset`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: u64,
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// The symbol in the buffer, highlighted while its documentation is shown.
    pub range: Option<Range<usize>>,
    pub documentation: Documentation,
}

#[derive(Default)]
struct State {
    next_id: u64,
    awaited: Option<Request>,
    requests: Vec<Request>,
    /// Some buffer holds a `Shown`, which the next command removes.
    showing: bool,
    /// `Editor::command_count` when last looked at.
    seen_commands: u64,
}

/// Documentation shown in a buffer while point stays where it was asked for.
struct Shown {
    point: usize,
    answer: Answer,
    /// All of it rather than its summary.
    full: bool,
}

impl Shown {
    fn anchor(&self) -> usize {
        self.answer
            .range
            .as_ref()
            .map_or(self.point, |range| range.start)
    }

    /// The blocks before the first heading past the start, like `# Examples`.
    fn summary_len(&self) -> usize {
        let blocks = &self.answer.documentation.blocks;
        blocks
            .iter()
            .skip(1)
            .position(|block| matches!(block, DocBlock::Heading(_)))
            .map_or(blocks.len(), |index| index + 1)
    }
}

fn state(editor: &mut Editor) -> &mut State {
    editor.plugin_state()
}

/// Point moved by the mouse hides the documentation.
fn shown(buffer: &Buffer) -> Option<&Shown> {
    buffer
        .local::<Shown>()
        .filter(|shown| shown.point == buffer.point())
}

fn close(editor: &mut Editor) {
    let state = state(editor);
    state.awaited = None;
    if mem::take(&mut state.showing) {
        for buffer in editor.buffers_mut() {
            buffer.remove_local::<Shown>();
        }
    }
}

/// Any command but asking again hides the documentation.
pub(crate) fn update(editor: &mut Editor) {
    let count = editor.command_count();
    if mem::replace(&mut state(editor).seen_commands, count) == count {
        return;
    }
    if editor.last_command() != Some(SHOW) {
        close(editor);
    }
}

/// Asks for the documentation of the symbol at point; asked again, shows all of it.
pub(crate) fn show(editor: &mut Editor) -> CommandResult {
    if editor.minibuffer().is_some() {
        return Err("Documentation is shown for buffers only".into());
    }
    let buffer = editor.window_buffer_mut();
    let point = buffer.point();
    if let Some(shown) = buffer
        .local_mut::<Shown>()
        .filter(|shown| shown.point == point)
    {
        shown.full = true;
        return Ok(());
    }
    close(editor);
    let state = state(editor);
    state.next_id += 1;
    let request = Request {
        id: state.next_id,
        offset: point,
    };
    state.awaited = Some(request);
    state.requests.push(request);
    Ok(())
}

pub(crate) fn hide(editor: &mut Editor) -> CommandResult {
    close(editor);
    Ok(())
}

pub fn take_requests(editor: &mut Editor) -> Vec<Request> {
    mem::take(&mut state(editor).requests)
}

/// Answers request `id`; anything but the newest request, or one asked before point
/// moved, is dropped.
pub fn deliver(editor: &mut Editor, id: u64, answer: Option<Answer>) {
    let Some(awaited) = state(editor).awaited.filter(|awaited| awaited.id == id) else {
        return;
    };
    state(editor).awaited = None;
    let Some(answer) = answer.filter(|answer| !answer.documentation.is_empty()) else {
        return editor.set_message("No documentation at point");
    };
    let buffer = editor.window_buffer_mut();
    if buffer.point() != awaited.offset {
        return;
    }
    buffer.set_local(Shown {
        point: awaited.offset,
        answer,
        full: false,
    });
    state(editor).showing = true;
}

pub(crate) fn is_shown(editor: &Editor) -> bool {
    editor.minibuffer().is_none() && shown(editor.window_buffer()).is_some()
}

pub(crate) fn popup(editor: &Editor) -> Option<Popup> {
    if editor.minibuffer().is_some() {
        return None;
    }
    let shown = shown(editor.window_buffer())?;
    let mut documentation = shown.answer.documentation.clone();
    let summary_len = shown.summary_len();
    let full = shown.full || summary_len == documentation.blocks.len();
    if !full {
        documentation.blocks.truncate(summary_len);
    }
    Some(Popup {
        anchor: shown.anchor(),
        rows: Vec::new(),
        selected: None,
        documentation: Some(documentation),
        hints: if full { FULL_HINTS } else { HINTS },
    })
}

/// The symbol the documentation is about.
pub(crate) fn highlight(buffer: &Buffer, range: Range<usize>) -> Vec<Span> {
    shown(buffer)
        .and_then(|shown| shown.answer.range.clone())
        .filter(|symbol| symbol.start < range.end && range.start < symbol.end)
        .map(|symbol| Span {
            range: symbol,
            face: Face::Highlight,
        })
        .into_iter()
        .collect()
}
