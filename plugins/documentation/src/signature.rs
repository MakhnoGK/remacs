use std::mem;

use remacs_core::{Buffer, Code, CommandResult, Documentation, Editor, Popup, SELF_INSERT_COMMAND};

pub(crate) const SHOW: &str = "signature-help";
pub(crate) const HIDE: &str = "hide-signature";
const KEYBOARD_QUIT: &str = "keyboard-quit";
const HINTS: &[(&str, &str)] = &[("s-p", "Next overload")];

/// Why the plugin asks the window buffer's backend for the signatures of the call at point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureTrigger {
    /// Asked with `signature-help`.
    Explicit,
    /// A character was typed; the backend answers those its language opens a call's
    /// arguments with, like `(`, and nothing for the others.
    Typed(char),
    /// While signatures are shown or asked for, a command ran, typing the character if
    /// one; the backend answers nothing once point is outside the call.
    Update(Option<char>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignatureRequest {
    pub id: u64,
    pub offset: usize,
    pub trigger: SignatureTrigger,
}

/// A call's signatures, each with the parameter at point as its emphasis, and the one
/// that fits the call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signatures {
    pub list: Vec<Code>,
    pub active: usize,
}

#[derive(Default)]
struct State {
    next_id: u64,
    awaited: Option<SignatureRequest>,
    requests: Vec<SignatureRequest>,
    /// Some buffer holds a `Shown`.
    showing: bool,
    /// `Editor::command_count` when last looked at.
    seen_commands: u64,
}

/// Signatures shown in a buffer, above the line point was on when they were first asked.
struct Shown {
    anchor: usize,
    /// Where point was after the last command; moved by the mouse, it hides them.
    point: usize,
    signatures: Signatures,
    /// The active one is the user's choice rather than the backend's.
    stepped: bool,
}

fn state(editor: &mut Editor) -> &mut State {
    editor.plugin_state()
}

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

fn request(editor: &mut Editor, trigger: SignatureTrigger) {
    let offset = editor.window_buffer().point();
    let state = state(editor);
    state.next_id += 1;
    let request = SignatureRequest {
        id: state.next_id,
        offset,
        trigger,
    };
    state.awaited = Some(request);
    state.requests.push(request);
}

/// Follows the call being typed: asks after a typed character, and after any command
/// while signatures are shown or on their way.
pub(crate) fn update(editor: &mut Editor) {
    let count = editor.command_count();
    if mem::replace(&mut state(editor).seen_commands, count) == count {
        return;
    }
    let command = editor.last_command();
    if matches!(command, Some(SHOW | HIDE)) {
        return;
    }
    let shown_here = editor.window_buffer().local::<Shown>().is_some();
    let switched = state(editor).showing && !shown_here;
    if editor.minibuffer().is_some() || command == Some(KEYBOARD_QUIT) || switched {
        return close(editor);
    }
    let following = shown_here || state(editor).awaited.is_some();
    // shortcut: a key pressed after a click shows them again until the answer for the new
    // point, a server's round trip; hide on the click itself if that is ever seen.
    let buffer = editor.window_buffer_mut();
    let point = buffer.point();
    if let Some(shown) = buffer.local_mut::<Shown>() {
        shown.point = point;
    }
    let buffer = editor.window_buffer();
    let typed = (command == Some(SELF_INSERT_COMMAND))
        .then(|| buffer.char_before(buffer.point()))
        .flatten();
    let trigger = match typed {
        _ if following => SignatureTrigger::Update(typed),
        Some(typed) => SignatureTrigger::Typed(typed),
        None => return,
    };
    request(editor, trigger);
}

/// Asks for the signatures of the call at point; asked again while several are shown,
/// shows the next.
pub(crate) fn show(editor: &mut Editor) -> CommandResult {
    if editor.minibuffer().is_some() {
        return Err("Signatures are shown for buffers only".into());
    }
    if let Some(shown) = editor.window_buffer_mut().local_mut::<Shown>()
        && shown.signatures.list.len() > 1
    {
        let signatures = &mut shown.signatures;
        signatures.active = (signatures.active + 1) % signatures.list.len();
        shown.stepped = true;
        return Ok(());
    }
    request(editor, SignatureTrigger::Explicit);
    Ok(())
}

pub(crate) fn hide(editor: &mut Editor) -> CommandResult {
    close(editor);
    Ok(())
}

pub fn take_signature_requests(editor: &mut Editor) -> Vec<SignatureRequest> {
    mem::take(&mut state(editor).requests)
}

/// Answers request `id`; anything but the newest request is dropped, and no signatures
/// hide those shown. The same call's signatures stay where they were shown, and the
/// overload the user stepped to stays chosen.
pub fn deliver_signatures(editor: &mut Editor, id: u64, signatures: Option<Signatures>) {
    let Some(awaited) = state(editor).awaited.filter(|awaited| awaited.id == id) else {
        return;
    };
    state(editor).awaited = None;
    let Some(mut signatures) = signatures.filter(|signatures| !signatures.list.is_empty()) else {
        close(editor);
        if awaited.trigger == SignatureTrigger::Explicit {
            editor.set_message("No signature at point");
        }
        return;
    };
    let buffer = editor.window_buffer_mut();
    if buffer.point() != awaited.offset {
        return;
    }
    let (mut anchor, mut stepped) = (awaited.offset, false);
    if let Some(shown) = buffer.local::<Shown>()
        && same_call(&shown.signatures, &signatures)
    {
        anchor = shown.anchor;
        if shown.stepped {
            signatures.active = shown.signatures.active;
            stepped = true;
        }
    }
    signatures.active = signatures.active.min(signatures.list.len() - 1);
    buffer.set_local(Shown {
        anchor,
        point: awaited.offset,
        signatures,
        stepped,
    });
    state(editor).showing = true;
}

fn same_call(shown: &Signatures, answered: &Signatures) -> bool {
    let labels = shown.list.iter().map(|code| &code.text);
    labels.eq(answered.list.iter().map(|code| &code.text))
}

pub(crate) fn is_shown(editor: &Editor) -> bool {
    editor.minibuffer().is_none() && shown(editor.window_buffer()).is_some()
}

/// The active signature above the line, with which of how many it is when there are more.
pub(crate) fn popup(editor: &Editor) -> Option<Popup> {
    if editor.minibuffer().is_some() {
        return None;
    }
    let buffer = editor.window_buffer();
    let shown = shown(buffer)?;
    let Signatures { list, active } = &shown.signatures;
    let overloads = list.len() > 1;
    Some(Popup {
        anchor: shown.anchor.min(buffer.text().len()),
        documentation: Some(Documentation {
            kind: if overloads {
                format!("{} of {}", active + 1, list.len())
            } else {
                String::new()
            },
            signature: Some(list[*active].clone()),
            ..Documentation::default()
        }),
        hints: if overloads { HINTS } else { &[] },
        above: true,
        ..Popup::default()
    })
}
