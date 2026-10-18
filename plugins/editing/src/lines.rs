//! Commands over whole lines of the region, or of the buffer: sorting, keeping and
//! flushing them, and lining up their text in columns, like Emacs `align-regexp`.

use std::collections::HashSet;
use std::ops::Range;

use regex::Regex;
use remacs_core::{Buffer, CommandResult, Completions, Editor, Prompt};

use crate::editing::writable_buffer;

const REGEXP_CATEGORY: &str = "regexp";

pub fn sort_lines(editor: &mut Editor) -> CommandResult {
    edit_lines(editor, |lines| lines.sort_unstable())
}

/// Leaves the first of equal lines, like Emacs `delete-duplicate-lines`.
pub fn delete_duplicate_lines(editor: &mut Editor) -> CommandResult {
    edit_lines(editor, |lines| {
        let mut seen = HashSet::new();
        lines.retain(|line| seen.insert(line.clone()));
    })
}

/// Reads a regexp and deletes the lines that do not match it, like Emacs `keep-lines`.
pub fn keep_lines(editor: &mut Editor) -> CommandResult {
    filter_lines(editor, "Keep lines matching", true)
}

/// Reads a regexp and deletes the lines that match it, like Emacs `flush-lines`.
pub fn flush_lines(editor: &mut Editor) -> CommandResult {
    filter_lines(editor, "Flush lines matching", false)
}

fn filter_lines(editor: &mut Editor, label: &str, keep: bool) -> CommandResult {
    writable_buffer(editor)?;
    read_regexp(editor, label, move |editor, regex| {
        edit_lines(editor, |lines| {
            lines.retain(|line| regex.is_match(line) == keep);
        })
    })
}

/// Reads a regexp and lines up its first match on each line of the region, one space
/// after the longest text before it.
pub fn align_regexp(editor: &mut Editor) -> CommandResult {
    start_aligning(editor, "Align regexp", false)
}

/// Like `align-regexp` for every match, in columns, as Emacs' with a prefix argument and
/// repeat.
pub fn align_regexp_repeat(editor: &mut Editor) -> CommandResult {
    start_aligning(editor, "Align regexp, every match", true)
}

fn start_aligning(editor: &mut Editor, label: &str, every_match: bool) -> CommandResult {
    writable_buffer(editor)?;
    editor.buffer().region().ok_or("No region to align")?;
    read_regexp(editor, label, move |editor, regex| {
        if regex.is_match("") {
            return Err("The regexp matches empty text".into());
        }
        edit_lines(editor, |lines| align(lines, &regex, every_match))
    })
}

fn read_regexp(
    editor: &mut Editor,
    label: &str,
    on_regex: impl FnOnce(&mut Editor, Regex) -> CommandResult + 'static,
) -> CommandResult {
    let prompt = Prompt {
        label: label.into(),
        category: REGEXP_CATEGORY,
        placeholder: "regexp",
        hints: &[("↵", "Run"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        |editor, input| {
            let regex = Regex::new(&input).map_err(|error| format!("Invalid regexp: {error}"))?;
            on_regex(editor, regex)
        },
    )
}

/// Pads the text before the matches of `regex` on `lines` so that they start in one
/// column, the first match or, with `every_match`, each in turn.
// shortcut: columns count characters, so a tab counts as one; expand tabs if users mix them.
fn align(lines: &mut [String], regex: &Regex, every_match: bool) {
    let mut searched_from = vec![0; lines.len()];
    loop {
        let matches: Vec<_> = lines
            .iter()
            .zip(&searched_from)
            .map(|(line, &from)| {
                // An empty match, as `\b` gives, would be found again after its padding.
                let found = regex
                    .find_at(line, from)
                    .filter(|found| !found.is_empty())?;
                let text_end = from
                    + line[from..found.start()]
                        .trim_end_matches([' ', '\t'])
                        .len();
                Some((text_end, found.start(), found.end()))
            })
            .collect();
        let Some(column) = lines
            .iter()
            .zip(&matches)
            .filter_map(|(line, found)| Some(line[..found.as_ref()?.0].chars().count()))
            .max()
        else {
            return;
        };
        for ((line, found), from) in lines.iter_mut().zip(matches).zip(&mut searched_from) {
            let Some((text_end, start, end)) = found else {
                *from = line.len();
                continue;
            };
            let padding = column + 1 - line[..text_end].chars().count();
            line.replace_range(text_end..start, &" ".repeat(padding));
            *from = end + text_end + padding - start;
        }
        if !every_match {
            return;
        }
    }
}

/// Replaces the lines of the region, or of the whole buffer, with what `edit` makes of
/// them.
fn edit_lines(editor: &mut Editor, edit: impl FnOnce(&mut Vec<String>)) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let mut range = whole_lines(buffer);
    let text = buffer.contents(range.clone());
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let before = lines.len();
    edit(&mut lines);
    let edited = lines.join("\n");
    // With none left, the lines go with their newline rather than leave a blank one.
    if lines.is_empty() && range.end < buffer.text().len() {
        range.end += 1;
    }
    if edited != text {
        buffer.apply_edits(vec![(range, edited)]);
    }
    let removed = before - lines.len();
    if removed > 0 {
        editor.set_message(format!(
            "Deleted {removed} line{}",
            if removed == 1 { "" } else { "s" }
        ));
    }
    Ok(())
}

/// The lines the region, or else the whole buffer, touches, from the start of its first
/// to the end of its last, newline left out; ending at the start of a line leaves that
/// line out, as the empty one after a final newline.
fn whole_lines(buffer: &Buffer) -> Range<usize> {
    let text = buffer.text();
    let (first, last) = lines_of(buffer, buffer.region().unwrap_or(0..text.len()));
    let start = text.line_start(first).unwrap_or(0);
    let end = text.line_range(last).map_or(text.len(), |line| line.end);
    start..end
}

/// The first and last lines `span` touches; ending at the start of a line leaves it out.
pub(crate) fn lines_of(buffer: &Buffer, span: Range<usize>) -> (usize, usize) {
    let text = buffer.text();
    let first = text.line_of(span.start);
    let last = text.line_of(span.end);
    if last > first && text.line_start(last) == Some(span.end) {
        (first, last - 1)
    } else {
        (first, last)
    }
}
