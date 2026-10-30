//! Where point was before each jump, to go back and forward through, like the back and
//! forward of IntelliJ and Emacs `pop-global-mark`. A jump is a move to another buffer, or
//! across `jump-lines` lines or more, seen at the end of a command with no prompt open: a
//! search or a list of definitions counts once, from where it started to where it ended.
//! Scrolling is reading on, not a jump, and neither is point following a program's output.

use std::mem;
use std::path::PathBuf;

use remacs_core::{Buffer, CommandResult, Editor};

use crate::NavigationSettings;

pub const NAVIGATE_BACK: &str = "navigate-back";
pub const NAVIGATE_FORWARD: &str = "navigate-forward";

#[derive(Clone, Debug, PartialEq, Eq)]
struct Place {
    buffer: String,
    path: Option<PathBuf>,
    // shortcut: a plain offset, not moved by later edits like the mark is; off by their
    // length after edits before it, fine for going back to roughly where point was.
    offset: usize,
    line: usize,
    /// The buffer's length then: output a program printed since changed it.
    text_len: usize,
}

impl Place {
    fn of(buffer: &Buffer) -> Self {
        Self {
            buffer: buffer.name().to_owned(),
            path: buffer.path().map(PathBuf::from),
            offset: buffer.point(),
            line: buffer.point_line(),
            text_len: buffer.text().len(),
        }
    }

    fn is_at(&self, buffer: &Buffer) -> bool {
        self.offset == buffer.point() && self.buffer == buffer.name()
    }
}

#[derive(Default)]
struct History {
    back: Vec<Place>,
    forward: Vec<Place>,
    /// Where point was when last seen with no prompt open.
    last: Option<Place>,
    /// [`Editor::command_count`] then.
    commands: u64,
}

/// Records the place point jumped from, if it jumped; run after every command and refresh.
pub(crate) fn update(editor: &mut Editor) {
    if editor.minibuffer().is_some() {
        return;
    }
    let commands = editor.command_count();
    let ran = mem::replace(&mut editor.plugin_state::<History>().commands, commands) != commands;
    record(editor, ran);
}

/// Takes where point is as the last place seen, keeping the one before to come back to
/// when point jumped from it: to another buffer, or across lines of one, but not as output
/// it follows does with no command since (`by_command` false), as in a program's console.
/// A click or an avy label moves it with the text unchanged. Where scrolling left point
/// stands for the last place, so reading on is no jump but going on from there is.
fn record(editor: &mut Editor, by_command: bool) {
    let buffer = editor.window_buffer();
    let text_len = buffer.text().len();
    // Where scrolling left point in the last place's buffer, with its line if that is the
    // window's: an answer may have shown another since, outside a command.
    let scrolled_to = editor.scrolled_to().and_then(|(name, offset)| {
        let last = editor.find_plugin_state::<History>()?.last.as_ref()?;
        let line = (name == buffer.name()).then(|| buffer.text().line_of(offset));
        (name == last.buffer).then_some((offset, line))
    });
    let unmoved = editor
        .find_plugin_state::<History>()
        .and_then(|history| history.last.as_ref())
        .is_some_and(|last| last.is_at(buffer));
    if unmoved {
        if let Some(last) = &mut editor.plugin_state::<History>().last {
            last.text_len = text_len;
        }
        return;
    }
    let here = Place::of(buffer);
    let settings = editor.plugin_state::<NavigationSettings>();
    let (jump_lines, history_size) = (settings.jump_lines, settings.history_size);
    let history = editor.plugin_state::<History>();
    let Some(mut last) = history.last.replace(here.clone()) else {
        return;
    };
    if let Some((offset, line)) = scrolled_to {
        last.offset = offset;
        if let Some(line) = line {
            last.line = line;
        }
    }
    let jumped = if last.buffer == here.buffer {
        (by_command || last.text_len == here.text_len)
            && last.line.abs_diff(here.line) >= jump_lines
    } else {
        true
    };
    if jumped {
        history.forward.clear();
        if history.back.len() == history_size {
            history.back.remove(0);
        }
        history.back.push(last);
    }
}

pub(crate) fn navigate_back(editor: &mut Editor) -> CommandResult {
    navigate(editor, |history| &mut history.back, |history| &mut history.forward)
        .ok_or_else(|| "At the start of the navigation history".into())
}

pub(crate) fn navigate_forward(editor: &mut Editor) -> CommandResult {
    navigate(editor, |history| &mut history.forward, |history| &mut history.back)
        .ok_or_else(|| "At the end of the navigation history".into())
}

/// Goes to the latest place of `from` that can still be shown, leaving where point was on
/// `to`; places in buffers since killed, which no file brings back, are dropped.
fn navigate(
    editor: &mut Editor,
    from: fn(&mut History) -> &mut Vec<Place>,
    to: fn(&mut History) -> &mut Vec<Place>,
) -> Option<()> {
    // A jump no command made, as a click is, is seen first, so that going back returns.
    record(editor, true);
    let here = Place::of(editor.window_buffer());
    while let Some(place) = from(editor.plugin_state::<History>()).pop() {
        if !show(editor, &place) {
            continue;
        }
        let arrived = Place::of(editor.window_buffer());
        let history = editor.plugin_state::<History>();
        to(history).push(here);
        history.last = Some(arrived);
        return Some(());
    }
    None
}

fn show(editor: &mut Editor, place: &Place) -> bool {
    let shown = match &place.path {
        // A file deleted or renamed since would open as a new, empty one.
        Some(path) => {
            let open = editor.buffers().iter().any(|buffer| buffer.path() == Some(path));
            (open || path.exists()) && editor.visit_file(path).is_ok()
        }
        None => editor.switch_to_buffer(&place.buffer),
    };
    if !shown {
        return false;
    }
    let buffer = editor.window_buffer_mut();
    let text = buffer.text();
    let offset = place.offset.min(text.len());
    let offset =
        if text.is_char_boundary(offset) { offset } else { text.previous_char_boundary(offset) };
    buffer.set_point(offset);
    buffer.deactivate_mark();
    let line = buffer.point_line();
    editor.show_line(line);
    true
}
