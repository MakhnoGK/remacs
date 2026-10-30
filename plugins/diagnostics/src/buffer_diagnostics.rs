use std::cmp::Reverse;
use std::mem;
use std::ops::Range;

use remacs_core::{Buffer, Editor, Face, Icon, Span};
use remacs_text::{Edit, Snapshot};

/// Ordered from least to most severe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    /// Most severe first, as the status pill lists them.
    pub const ALL: [Severity; 3] = [Severity::Error, Severity::Warning, Severity::Info];

    pub fn icon(self) -> Icon {
        match self {
            Severity::Error => Icon::Error,
            Severity::Warning => Icon::Warning,
            Severity::Info => Icon::Info,
        }
    }

    fn face(self) -> Face {
        match self {
            Severity::Error => Face::Error,
            Severity::Warning => Face::Warning,
            Severity::Info => Face::Info,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Byte offsets; empty where something is missing, such as a `;`.
    pub range: Range<usize>,
    pub severity: Severity,
    pub message: String,
    /// Who reported it and the rule, e.g. `rustc · E0599`.
    pub source: String,
}

impl Diagnostic {
    pub fn summary(&self) -> &str {
        self.message.lines().next().unwrap_or_default()
    }
}

/// A buffer's diagnostics from all its checkers, sorted by position, and the text their
/// ranges refer to.
pub(crate) struct BufferDiagnostics {
    snapshot: Snapshot,
    /// Who checks the buffer, such as language servers, even with nothing to report.
    checkers: Vec<&'static str>,
    pub list: Vec<Diagnostic>,
    /// The checker of each diagnostic in `list`; a report replaces only its own.
    reporters: Vec<&'static str>,
}

impl BufferDiagnostics {
    /// Moves the ranges along with the edits made since they were reported, so they
    /// stay on their text until the checker reports again.
    fn rebase(&mut self, text: Snapshot) {
        move_ranges(&mut self.list, &self.snapshot, &text);
        self.snapshot = text;
    }

    /// Replaces `checker`'s diagnostics; both refer to the snapshot.
    fn replace(&mut self, checker: &'static str, diagnostics: Vec<Diagnostic>) {
        if !self.checkers.contains(&checker) {
            self.checkers.push(checker);
        }
        let mut merged: Vec<_> = mem::take(&mut self.list)
            .into_iter()
            .zip(mem::take(&mut self.reporters))
            .filter(|&(_, reporter)| reporter != checker)
            .chain(diagnostics.into_iter().map(|diagnostic| (diagnostic, checker)))
            .collect();
        merged
            .sort_by_key(|(diagnostic, _)| (diagnostic.range.start, Reverse(diagnostic.severity)));
        (self.list, self.reporters) = merged.into_iter().unzip();
    }

    /// Those whose marked text holds `offset`, as they are underlined.
    pub fn at<'a>(
        &'a self,
        text: &'a Snapshot,
        offset: usize,
    ) -> impl Iterator<Item = (Range<usize>, &'a Diagnostic)> {
        // An empty range at a line's end marks the character before it.
        let candidates =
            self.list.partition_point(|diagnostic| diagnostic.range.start <= offset + 1);
        self.list[..candidates]
            .iter()
            .map(|diagnostic| (marked_range(text, &diagnostic.range), diagnostic))
            .filter(move |(range, _)| range.contains(&offset))
    }

    pub fn count(&self, severity: Severity) -> usize {
        self.list.iter().filter(|diagnostic| diagnostic.severity == severity).count()
    }
}

/// Replaces what `checker` reported for the buffer; the ranges refer to `text`, which may
/// be older than the buffer's text. Other checkers' diagnostics stay.
pub fn publish(
    buffer: &mut Buffer,
    checker: &'static str,
    text: Snapshot,
    mut diagnostics: Vec<Diagnostic>,
) {
    let current = buffer.text().snapshot();
    move_ranges(&mut diagnostics, &text, &current);
    if buffer.local::<BufferDiagnostics>().is_none() {
        buffer.set_local(BufferDiagnostics {
            snapshot: current.clone(),
            checkers: Vec::new(),
            list: Vec::new(),
            reporters: Vec::new(),
        });
    }
    let state = buffer.local_mut::<BufferDiagnostics>().expect("the state was just set");
    state.rebase(current);
    state.replace(checker, diagnostics);
}

/// Forgets what `checker` reported, e.g. when it no longer checks the buffer; with no
/// checker left, the buffer has no diagnostics at all.
pub fn clear(buffer: &mut Buffer, checker: &'static str) {
    let Some(state) = buffer.local_mut::<BufferDiagnostics>() else {
        return;
    };
    state.replace(checker, Vec::new());
    state.checkers.retain(|&other| other != checker);
    if state.checkers.is_empty() {
        buffer.remove_local::<BufferDiagnostics>();
    }
}

pub(crate) fn rebase(editor: &mut Editor) {
    let buffer = editor.window_buffer_mut();
    let text = buffer.text().snapshot();
    if let Some(state) = buffer.local_mut::<BufferDiagnostics>() {
        state.rebase(text);
    }
}

/// Underlines for `bounds`; where problems overlap, the most severe one shows.
pub(crate) fn underline(buffer: &Buffer, bounds: Range<usize>) -> Vec<Span> {
    let Some(state) = buffer.local::<BufferDiagnostics>() else {
        return Vec::new();
    };
    let text = buffer.text();
    let candidates = state.list.partition_point(|diagnostic| diagnostic.range.start <= bounds.end);
    let marked: Vec<_> = state.list[..candidates]
        .iter()
        .filter(|diagnostic| diagnostic.range.end >= bounds.start)
        .map(|diagnostic| (marked_range(text, &diagnostic.range), diagnostic.severity))
        .filter(|(range, _)| range.start < bounds.end && range.end > bounds.start)
        .collect();
    flatten(&marked, bounds)
}

/// An empty range still marks one character: the next one, or the last of its line.
fn marked_range(text: &Snapshot, range: &Range<usize>) -> Range<usize> {
    let range = range.start.min(text.len())..range.end.min(text.len());
    if !range.is_empty() {
        return range;
    }
    let at_line_end =
        text.chunks(range.start..range.start + 1).next().is_none_or(|chunk| chunk[0] == b'\n');
    if at_line_end {
        text.previous_char_boundary(range.start)..range.start
    } else {
        range.start..text.next_char_boundary(range.start)
    }
}

fn flatten(marked: &[(Range<usize>, Severity)], bounds: Range<usize>) -> Vec<Span> {
    let mut edges: Vec<usize> = marked
        .iter()
        .flat_map(|(range, _)| [range.start, range.end])
        .map(|offset| offset.clamp(bounds.start, bounds.end))
        .collect();
    edges.sort_unstable();
    edges.dedup();
    let mut spans: Vec<Span> = Vec::new();
    for edge in edges.windows(2) {
        let (start, end) = (edge[0], edge[1]);
        let Some(severity) = marked
            .iter()
            .filter(|(range, _)| range.start <= start && range.end >= end)
            .map(|&(_, severity)| severity)
            .max()
        else {
            continue;
        };
        let face = severity.face();
        match spans.last_mut() {
            Some(last) if last.face == face && last.range.end == start => last.range.end = end,
            _ => spans.push(Span { range: start..end, face }),
        }
    }
    spans
}

/// Moves the ranges, which refer to `from`, onto `to`.
fn move_ranges(diagnostics: &mut [Diagnostic], from: &Snapshot, to: &Snapshot) {
    let Some(edit) = to.edit_from(from) else {
        return;
    };
    for diagnostic in diagnostics {
        let range = &diagnostic.range;
        diagnostic.range =
            shift(range.start, edit, edit.start)..shift(range.end, edit, edit.new_end);
    }
}

/// Where `offset` ends up after `edit`; offsets inside the replaced text go to `inside`.
fn shift(offset: usize, edit: Edit, inside: usize) -> usize {
    if offset <= edit.start {
        offset
    } else if offset >= edit.old_end {
        offset - edit.old_end + edit.new_end
    } else {
        inside
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_severe_overlapping_problem_shows() {
        let marked = [(0..10, Severity::Info), (2..4, Severity::Error), (3..6, Severity::Warning)];
        let spans: Vec<_> =
            flatten(&marked, 1..8).into_iter().map(|span| (span.range, span.face)).collect();
        assert_eq!(
            spans,
            [(1..2, Face::Info), (2..4, Face::Error), (4..6, Face::Warning), (6..8, Face::Info),]
        );
    }
}
