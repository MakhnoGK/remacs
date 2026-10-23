//! Going to a line typed by its number, and a column after a colon, like Emacs
//! `goto-line`; both count from 1, as the status bar shows them.

use remacs_core::{CommandResult, Completions, Editor, Prompt};

pub const GOTO_LINE: &str = "goto-line";
const CATEGORY: &str = "line";

pub(crate) fn goto_line(editor: &mut Editor) -> CommandResult {
    let line_count = editor.window_buffer().text().line_count();
    let prompt = Prompt {
        label: format!("Go to line (1–{line_count})"),
        category: CATEGORY,
        placeholder: "line, or line:column",
        hints: &[("↵", "Go to"), ("M-p M-n", "History"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        |editor, answer| {
            let (line, column) = parse(&answer).ok_or("Type a line, or line:column")?;
            editor.window_buffer_mut().push_mark();
            editor.set_point_at(line, column);
            let line = editor.window_buffer().point_line();
            editor.show_line(line);
            Ok(())
        },
    )
}

/// The 0-based line and column of `line` or `line:column`; 0 counts as 1, and past the
/// end is clamped by the caller.
fn parse(answer: &str) -> Option<(usize, usize)> {
    let mut parts = answer.trim().splitn(2, ':');
    let number = |text: &str| {
        text.trim()
            .parse::<usize>()
            .ok()
            .map(|n| n.saturating_sub(1))
    };
    Some((
        number(parts.next()?)?,
        parts.next().map_or(Some(0), number)?,
    ))
}
