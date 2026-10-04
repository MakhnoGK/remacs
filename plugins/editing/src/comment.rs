use std::ops::Range;

use remacs_core::{Buffer, CommandResult, CommentSyntax, Editor};

use crate::editing::writable_buffer;

/// Comments out the region's lines, or point's line, or uncomments them when every one
/// is commented already. Blank lines are left alone; an active region stays active, so
/// the toggle can be repeated.
pub fn comment_line(editor: &mut Editor) -> CommandResult {
    let syntax = editor
        .buffer_mode()
        .and_then(|mode| mode.comment)
        .ok_or("No comment syntax in this mode")?;
    let buffer = writable_buffer(editor)?;
    let region = buffer.region();
    let lines = filled_lines(buffer, region.clone());
    if lines.is_empty() {
        return Ok(());
    }
    let edits = if lines.iter().all(|(_, line)| is_commented(line, syntax)) {
        lines
            .iter()
            .flat_map(|(range, line)| uncomment(range.start, line, syntax))
            .collect()
    } else {
        let column = lines
            .iter()
            .map(|(_, line)| blanks(line))
            .min()
            .unwrap_or(0);
        lines
            .iter()
            .flat_map(|(range, line)| comment(range.start, line, column, syntax))
            .collect()
    };
    buffer.apply_edits(edits);
    if region.is_some() {
        buffer.activate_mark();
    }
    Ok(())
}

/// The lines the region touches, or point's line, that hold more than blanks; a region
/// ending at the start of a line leaves that line out.
fn filled_lines(buffer: &Buffer, region: Option<Range<usize>>) -> Vec<(Range<usize>, String)> {
    let text = buffer.text();
    let span = region.unwrap_or(buffer.point()..buffer.point());
    let first = text.line_of(span.start);
    let mut last = text.line_of(span.end);
    if last > first && text.line_start(last) == Some(span.end) {
        last -= 1;
    }
    (first..=last)
        .filter_map(|line| text.line_range(line))
        .map(|range| (range.clone(), buffer.contents(range)))
        .filter(|(_, line)| !line.trim().is_empty())
        .collect()
}

fn is_commented(line: &str, syntax: CommentSyntax) -> bool {
    line.trim_start().starts_with(syntax.start.trim_end())
        && line.trim_end().ends_with(syntax.end.trim_start())
}

fn comment(
    line_start: usize,
    line: &str,
    column: usize,
    syntax: CommentSyntax,
) -> Vec<(Range<usize>, String)> {
    let start = line_start + column;
    let mut edits = vec![(start..start, syntax.start.to_owned())];
    if !syntax.end.is_empty() {
        let end = line_start + line.trim_end().len();
        edits.push((end..end, syntax.end.to_owned()));
    }
    edits
}

/// Removes the comment tokens and the one space of padding beside each, if present.
fn uncomment(line_start: usize, line: &str, syntax: CommentSyntax) -> Vec<(Range<usize>, String)> {
    let token = syntax.start.trim_end();
    let start = blanks(line);
    let mut start_end = start + token.len();
    if line[start_end..].starts_with(' ') {
        start_end += 1;
    }
    let mut edits = vec![(line_start + start..line_start + start_end, String::new())];
    let token = syntax.end.trim_start();
    let end = line.trim_end().len();
    // A line as short as `/*/` holds one token for both ends.
    if !token.is_empty() && end >= start_end + token.len() {
        let mut end_start = end - token.len();
        if end_start > start_end && line[..end_start].ends_with(' ') {
            end_start -= 1;
        }
        edits.push((line_start + end_start..line_start + end, String::new()));
    }
    edits
}

fn blanks(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}
