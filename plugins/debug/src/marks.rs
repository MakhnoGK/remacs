//! Breakpoints and the paused program's line in the gutter, breakpoints kept where they
//! are as their files are edited.

use std::ops::Range;
use std::path::PathBuf;

use remacs_core::{BreakpointIcon, CommandResult, Editor, GutterIcon, GutterMark};
use remacs_text::Snapshot;

use crate::breakpoints::Breakpoint;
use crate::session::{self, Debugger, current_frame, debugger};

pub const TOGGLE_BREAKPOINT: &str = "debug-toggle-breakpoint";

/// Modes of languages that can be debugged; their files get a column for breakpoints.
const DEBUGGABLE_MODES: &[&str] = &["Rust", "C", "C++", "TypeScript", "TSX", "JavaScript", "JSX"];

pub fn gutter_marks(editor: &Editor, lines: Range<usize>) -> Option<Vec<GutterMark>> {
    let buffer = editor.window_buffer();
    let path = buffer.path()?;
    if !DEBUGGABLE_MODES.contains(&buffer.mode()) {
        return None;
    }
    let mut marks: Vec<GutterMark> = editor
        .find_plugin_state::<Debugger>()
        .map(|debugger| debugger.breakpoints.of(path))
        .unwrap_or_default()
        .iter()
        .filter(|breakpoint| lines.contains(&breakpoint.line))
        .map(|breakpoint| GutterMark {
            line: breakpoint.line,
            icon: GutterIcon::Breakpoint(icon(breakpoint)),
            command: TOGGLE_BREAKPOINT,
        })
        .collect();
    if let Some(frame) = current_frame(editor)
        && frame.path.as_deref() == Some(path)
        && lines.contains(&(frame.line.saturating_sub(1)))
    {
        marks.push(GutterMark {
            line: frame.line - 1,
            icon: GutterIcon::ExecutionPoint,
            command: TOGGLE_BREAKPOINT,
        });
    }
    Some(marks)
}

pub fn icon(breakpoint: &Breakpoint) -> BreakpointIcon {
    if !breakpoint.enabled {
        BreakpointIcon::Disabled
    } else if breakpoint.verified == Some(false) {
        BreakpointIcon::Unverified
    } else if breakpoint.log_message.is_some() {
        BreakpointIcon::Log
    } else if breakpoint.condition.is_some() || breakpoint.hit_condition.is_some() {
        BreakpointIcon::Conditional
    } else {
        BreakpointIcon::Plain
    }
}

/// The window buffer's file and point's line.
pub fn point_line(editor: &Editor) -> Result<(PathBuf, usize), &'static str> {
    let buffer = editor.window_buffer();
    let path = buffer.path().ok_or("Breakpoints go in files")?;
    Ok((path.to_path_buf(), buffer.point_line()))
}

pub fn toggle_breakpoint(editor: &mut Editor) -> CommandResult {
    let (path, line) = point_line(editor)?;
    let breakpoints = &mut debugger(editor).breakpoints;
    let added = breakpoints.toggle(&path, line);
    breakpoints.save();
    session::breakpoints_changed(editor, &path);
    editor.set_message(if added {
        format!("Breakpoint at line {}", line + 1)
    } else {
        format!("Removed the breakpoint at line {}", line + 1)
    });
    Ok(())
}

/// Moves breakpoints along the edits made to their files since the last command, and
/// tells the adapters where they are now.
pub fn follow_edits(editor: &mut Editor) {
    let Some(debugger) = editor.find_plugin_state::<Debugger>() else {
        return;
    };
    let texts: Vec<(PathBuf, Snapshot)> = editor
        .buffers()
        .iter()
        .filter_map(|buffer| {
            let path = buffer.path()?;
            debugger
                .breakpoints
                .files
                .contains_key(path)
                .then(|| (path.to_path_buf(), buffer.text().snapshot()))
        })
        .collect();
    let breakpoints = &mut self::debugger(editor).breakpoints;
    let moved: Vec<PathBuf> = texts
        .into_iter()
        .filter_map(|(path, text)| breakpoints.follow(&path, text).then_some(path))
        .collect();
    if moved.is_empty() {
        return;
    }
    breakpoints.save();
    for path in moved {
        session::breakpoints_changed(editor, &path);
    }
}
