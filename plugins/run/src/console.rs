//! The buffer a run's output goes to, like Emacs `*compilation*`: read-only, colored as
//! the program asked, its `file:line`s links.

use std::ops::Range;
use std::path::{Path, PathBuf};

use remacs_core::{Buffer, ConsoleColor, ConsoleStyle, Face, Span};
use remacs_text::Text;

use crate::ansi::{Parser, Segment};
use remacs_files::links::{Link, Target, links_in};

/// The mode of console buffers, shown in the status pill.
pub const CONSOLE_MODE: &str = "Run";

/// Lines past the limit dropped at once, a tenth of it, so that the text is not rebuilt
/// for every line.
const TRIM_SLACK_DIVISOR: usize = 10;

/// What a console buffer keeps besides its text.
pub struct Console {
    pub session: u64,
    /// Where relative paths in the output are looked for: the program's directory, then
    /// the project root.
    pub directories: [PathBuf; 2],
    parser: Parser,
    /// Spans of the text in a style of their own, sorted and apart.
    styles: Vec<(Range<usize>, ConsoleStyle)>,
    /// A carriage return came: the next text replaces the last line.
    redraw_line: bool,
    /// Where the `file:line` last visited by `compile-next-error` starts, like Emacs's
    /// current error.
    pub location: Option<usize>,
}

pub fn new_buffer(name: String, session: u64, cwd: &Path, root: &Path) -> Buffer {
    let mut buffer = Buffer::new(name, Text::default(), CONSOLE_MODE)
        .with_directory(cwd.to_path_buf())
        .read_only();
    buffer.set_local(Console {
        session,
        directories: [cwd.to_path_buf(), root.to_path_buf()],
        parser: Parser::default(),
        styles: Vec::new(),
        redraw_line: false,
        location: None,
    });
    buffer
}

/// Empties a console for another run.
pub fn clear(buffer: &mut Buffer) {
    buffer.replace_text(Text::default());
    buffer.set_point(0);
    if let Some(console) = buffer.local_mut::<Console>() {
        console.parser = Parser::default();
        console.styles.clear();
        console.redraw_line = false;
        console.location = None;
    }
}

/// Appends a program's output, keeping at most about `max_lines` lines.
pub fn append_output(buffer: &mut Buffer, bytes: &[u8], max_lines: usize) {
    let Some(console) = buffer.local_mut::<Console>() else {
        return;
    };
    let segments = console.parser.feed(bytes);
    for segment in segments {
        match segment {
            Segment::Return => set_redraw(buffer, true),
            Segment::Text(text, style) => {
                if take_redraw(buffer) && !text.starts_with('\n') {
                    let text = buffer.text();
                    let last_line = text.line_count() - 1;
                    let start = text.line_start(last_line).unwrap_or(text.len());
                    truncate(buffer, start);
                }
                insert(buffer, &text, style);
            }
        }
    }
    trim(buffer, max_lines);
}

/// Appends a line of its own, e.g. the command run, after the output so far.
pub fn append_note(buffer: &mut Buffer, note: &str, style: ConsoleStyle) {
    set_redraw(buffer, false);
    let text = buffer.text();
    let needs_newline =
        !text.is_empty() && !text.bytes_in(text.len() - 1..text.len()).ends_with(b"\n");
    if needs_newline {
        insert(buffer, "\n", ConsoleStyle::default());
    }
    insert(buffer, note, style);
    insert(buffer, "\n", ConsoleStyle::default());
}

pub fn dim() -> ConsoleStyle {
    ConsoleStyle {
        dim: true,
        ..ConsoleStyle::default()
    }
}

pub fn colored(index: u8) -> ConsoleStyle {
    ConsoleStyle {
        color: Some(ConsoleColor::Indexed(index)),
        ..ConsoleStyle::default()
    }
}

fn set_redraw(buffer: &mut Buffer, redraw: bool) {
    if let Some(console) = buffer.local_mut::<Console>() {
        console.redraw_line = redraw;
    }
}

fn take_redraw(buffer: &mut Buffer) -> bool {
    buffer
        .local_mut::<Console>()
        .is_some_and(|console| std::mem::take(&mut console.redraw_line))
}

fn insert(buffer: &mut Buffer, text: &str, style: ConsoleStyle) {
    let start = buffer.text().len();
    buffer.append_output(text);
    let end = buffer.text().len();
    let Some(console) = buffer.local_mut::<Console>() else {
        return;
    };
    if style == ConsoleStyle::default() || start == end {
        return;
    }
    match console.styles.last_mut() {
        Some((last, last_style)) if last.end == start && *last_style == style => last.end = end,
        _ => console.styles.push((start..end, style)),
    }
}

fn truncate(buffer: &mut Buffer, len: usize) {
    buffer.truncate_output(len);
    if let Some(console) = buffer.local_mut::<Console>() {
        console.styles.retain_mut(|(range, _)| {
            range.end = range.end.min(len);
            range.start < range.end
        });
    }
}

/// Drops the oldest lines once there are a tenth more than `max_lines`.
fn trim(buffer: &mut Buffer, max_lines: usize) {
    let text = buffer.text();
    let line_count = text.line_count();
    if line_count <= max_lines + max_lines / TRIM_SLACK_DIVISOR {
        return;
    }
    let cut = text.line_start(line_count - max_lines).unwrap_or(0);
    let kept = buffer.contents(cut..text.len());
    let point = buffer.point().saturating_sub(cut);
    buffer.replace_text(Text::from(kept.as_str()));
    buffer.set_point(point);
    if let Some(console) = buffer.local_mut::<Console>() {
        console.styles.retain_mut(|(range, _)| {
            *range = range.start.saturating_sub(cut)..range.end.saturating_sub(cut);
            range.start < range.end
        });
        console.location = console
            .location
            .and_then(|location| location.checked_sub(cut));
    }
}

/// The program's styles over `range` of a console buffer.
pub fn style_spans(buffer: &Buffer, range: Range<usize>) -> Vec<Span> {
    let Some(console) = buffer.local::<Console>() else {
        return Vec::new();
    };
    let first = console
        .styles
        .partition_point(|(styled, _)| styled.end <= range.start);
    console.styles[first..]
        .iter()
        .take_while(|(styled, _)| styled.start < range.end)
        .map(|(styled, style)| Span {
            range: styled.start.max(range.start)..styled.end.min(range.end),
            face: Face::Console(*style),
        })
        .collect()
}

/// The links over `range` of a console buffer.
pub fn link_spans(buffer: &Buffer, range: Range<usize>) -> Vec<Span> {
    if buffer.local::<Console>().is_none() || range.is_empty() {
        return Vec::new();
    }
    let text = buffer.text();
    let lines = text.line_of(range.start)..text.line_of(range.end - 1) + 1;
    lines
        .flat_map(|line| line_links(buffer, line))
        .filter(|(span, _)| span.start < range.end && span.end > range.start)
        .map(|(span, _)| Span {
            range: span.start.max(range.start)..span.end.min(range.end),
            face: Face::Link,
        })
        .collect()
}

/// The links of `line`, by buffer offsets.
fn line_links(buffer: &Buffer, line: usize) -> Vec<(Range<usize>, Target)> {
    let Some(range) = buffer.text().line_range(line) else {
        return Vec::new();
    };
    links_in(&buffer.contents(range.clone()))
        .into_iter()
        .map(
            |Link {
                 range: link,
                 target,
             }| { (range.start + link.start..range.start + link.end, target) },
        )
        .collect()
}

/// The link at `offset`, or ending there.
pub fn link_at(buffer: &Buffer, offset: usize) -> Option<Target> {
    let line = buffer.text().line_of(offset);
    line_links(buffer, line)
        .into_iter()
        .find(|(range, _)| range.start <= offset && offset <= range.end)
        .map(|(_, target)| target)
}

/// The next `file:line` link starting after `offset`, from the first when there is no
/// offset, or the previous one before it: where it starts, and where it leads.
pub fn next_location(
    buffer: &Buffer,
    offset: Option<usize>,
    forward: bool,
) -> Option<(usize, Target)> {
    let text = buffer.text();
    let files = |line: usize| {
        line_links(buffer, line)
            .into_iter()
            .filter(|(_, target)| matches!(target, Target::File { .. }))
            .map(|(range, target)| (range.start, target))
    };
    match (offset, forward) {
        (None, true) => (0..text.line_count()).flat_map(files).next(),
        (Some(offset), true) => (text.line_of(offset)..text.line_count())
            .flat_map(files)
            .find(|&(start, _)| start > offset),
        (None, false) => None,
        (Some(offset), false) => (0..=text.line_of(offset))
            .rev()
            .flat_map(|line| files(line).rev())
            .find(|&(start, _)| start < offset),
    }
}

/// Where the next link after `offset` starts, or the previous one before it.
pub fn next_link(buffer: &Buffer, offset: usize, forward: bool) -> Option<usize> {
    let text = buffer.text();
    let line = text.line_of(offset);
    let starts = |line: usize| {
        line_links(buffer, line)
            .into_iter()
            .map(|(range, _)| range.start)
    };
    if forward {
        (line..text.line_count())
            .flat_map(starts)
            .find(|&start| start > offset)
    } else {
        (0..=line)
            .rev()
            .flat_map(|line| starts(line).rev())
            .find(|&start| start < offset)
    }
}
