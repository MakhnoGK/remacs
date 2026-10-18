use remacs_core::{Candidate, CommandResult, Editor, Prompt, filter_candidates};

use crate::editing::writable_buffer;
use crate::motion::{
    at_end_of_buffer, line_end, paragraph_end, paragraph_start, word_end, word_start,
};

pub const SET_MARK_COMMAND: &str = "set-mark-command";
pub const POP_TO_MARK_COMMAND: &str = "pop-to-mark-command";
pub const YANK: &str = "yank";
pub const YANK_POP: &str = "yank-pop";

const NO_MARK: &str = "The mark is not set now, so there is no region";
const PREVIEW_CHARS: usize = 120;

/// Set while `pop-to-mark-command` and the `set-mark-command`s right after it pop marks.
#[derive(Default)]
struct PoppingMarks(bool);

/// Sets the mark at point, the one before onto the mark ring; repeated, it deactivates it
/// again; right after `pop-to-mark-command`, it pops the next mark, like Emacs with
/// `set-mark-command-repeat-pop`.
pub fn set_mark_command(editor: &mut Editor) -> CommandResult {
    let after_pop = matches!(
        editor.last_command(),
        Some(SET_MARK_COMMAND | POP_TO_MARK_COMMAND)
    ) && editor.plugin_state::<PoppingMarks>().0;
    if after_pop {
        return pop_mark(editor);
    }
    editor.plugin_state::<PoppingMarks>().0 = false;
    let repeated = editor.last_command() == Some(SET_MARK_COMMAND);
    let buffer = editor.buffer_mut();
    if repeated && buffer.is_mark_active() {
        buffer.deactivate_mark();
        editor.set_message("Mark deactivated");
        return Ok(());
    }
    buffer.push_mark();
    buffer.activate_mark();
    editor.set_message("Mark set");
    Ok(())
}

/// Goes to the mark, then, run again, to the marks before it, like Emacs `C-u C-SPC`.
pub fn pop_to_mark_command(editor: &mut Editor) -> CommandResult {
    editor.plugin_state::<PoppingMarks>().0 = true;
    pop_mark(editor)
}

fn pop_mark(editor: &mut Editor) -> CommandResult {
    if editor.buffer_mut().pop_mark() {
        Ok(())
    } else {
        Err("No mark set in this buffer".into())
    }
}

pub fn exchange_point_and_mark(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    let mark = buffer.mark().ok_or("No mark set in this buffer")?;
    buffer.set_mark(buffer.point());
    buffer.set_point(mark);
    buffer.activate_mark();
    Ok(())
}

/// Selects the buffer, like Emacs, which pushes point onto the mark ring before.
pub fn mark_whole_buffer(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    buffer.push_mark();
    buffer.set_point(buffer.text().len());
    buffer.push_mark();
    buffer.set_point(0);
    buffer.activate_mark();
    Ok(())
}

pub fn kill_region(editor: &mut Editor) -> CommandResult {
    writable_buffer(editor)?;
    let region = editor.buffer().mark_range().ok_or(NO_MARK)?;
    editor.kill(region);
    Ok(())
}

pub fn kill_ring_save(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    let region = buffer.mark_range().ok_or(NO_MARK)?;
    let text = buffer.contents(region);
    buffer.deactivate_mark();
    editor.kill_ring.push(text);
    Ok(())
}

/// Kills the rest of the line, or through the newline when only blanks remain.
pub fn kill_line(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let (point, line_end, len) = (buffer.point(), line_end(buffer), buffer.text().len());
    let rest_is_blank = buffer.contents(point..line_end).trim().is_empty();
    let end = if rest_is_blank && line_end < len {
        line_end + 1
    } else {
        line_end
    };
    if end == point {
        return at_end_of_buffer(editor);
    }
    editor.kill(point..end);
    Ok(())
}

pub fn kill_word(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let (point, end) = (buffer.point(), word_end(buffer, buffer.point()));
    editor.kill(point..end);
    Ok(())
}

pub fn backward_kill_word(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let (start, point) = (word_start(buffer, buffer.point()), buffer.point());
    editor.kill(start..point);
    Ok(())
}

/// Kills to the end of the paragraph, like Emacs `kill-paragraph`.
pub fn kill_paragraph(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let (point, end) = (buffer.point(), paragraph_end(buffer, buffer.point_line()));
    if end == point {
        return at_end_of_buffer(editor);
    }
    editor.kill(point..end);
    Ok(())
}

/// Selects the paragraph at point, or the next one from a blank line, point before it
/// and mark after it, like Emacs `mark-paragraph`.
pub fn mark_paragraph(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    let end = paragraph_end(buffer, buffer.point_line());
    let end_line = buffer.text().line_of(end);
    let last_line = if end == buffer.text().len() {
        end_line
    } else {
        end_line.saturating_sub(1)
    };
    buffer.set_mark(end);
    buffer.set_point(paragraph_start(buffer, last_line));
    buffer.activate_mark();
    Ok(())
}

pub fn yank(editor: &mut Editor) -> CommandResult {
    writable_buffer(editor)?;
    let text = editor
        .kill_ring
        .newest()
        .ok_or("Kill ring is empty")?
        .to_owned();
    insert_yanked(editor, &text);
    Ok(())
}

/// Replaces the text just yanked with the previous kill; elsewhere, picks a kill to
/// insert in the minibuffer, as in Emacs 28.
pub fn yank_pop(editor: &mut Editor) -> CommandResult {
    writable_buffer(editor)?;
    if !matches!(editor.last_command(), Some(YANK | YANK_POP)) {
        return yank_from_kill_ring(editor);
    }
    let text = editor
        .kill_ring
        .rotate()
        .ok_or("Kill ring is empty")?
        .to_owned();
    let buffer = editor.buffer_mut();
    let yanked = buffer.mark_range().ok_or(NO_MARK)?;
    buffer.delete(yanked.clone());
    buffer.set_point(yanked.start);
    insert_yanked(editor, &text);
    Ok(())
}

fn yank_from_kill_ring(editor: &mut Editor) -> CommandResult {
    if editor.kill_ring.is_empty() {
        return Err("Kill ring is empty".into());
    }
    let prompt = Prompt {
        label: "Yank".into(),
        category: "kill-ring",
        private: true,
        require_match: true,
        hints: &[("↵", "Insert"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |editor, input| {
            let candidates = editor
                .kill_ring
                .entries()
                .map(|entry| Candidate {
                    label: preview(entry),
                    ..Candidate::new(entry)
                })
                .collect();
            filter_candidates(input, candidates, false)
        },
        |editor, text| {
            insert_yanked(editor, &text);
            Ok(())
        },
    )
}

/// Inserts at point and leaves the mark at the start, so `yank-pop` can replace it.
fn insert_yanked(editor: &mut Editor, text: &str) {
    let buffer = editor.buffer_mut();
    let start = buffer.point();
    buffer.insert(text);
    buffer.set_mark(start);
}

/// One line of a kill, newlines shown as `⏎`.
fn preview(text: &str) -> String {
    text.trim()
        .chars()
        .take(PREVIEW_CHARS)
        .map(|character| if character == '\n' { '⏎' } else { character })
        .collect()
}
