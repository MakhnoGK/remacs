//! Lines hidden by folds, like Emacs hideshow: a fold hides whole lines after its header
//! line, a block's body, until it is unfolded, point goes into it or an edit touches it.

use std::ops::Range;

use remacs_text::Snapshot;

use crate::buffer::Buffer;

/// A buffer's folds, in a buffer local, with the text they were placed in.
struct Folds {
    snapshot: Snapshot,
    /// In order of their starts; folds within folds too.
    folds: Vec<Fold>,
    shown: ShownLines,
}

/// The lines one fold hides, and their bytes: from the start of the first to the end of
/// the last.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fold {
    bytes: Range<usize>,
    lines: Range<usize>,
}

impl Folds {
    fn new(snapshot: Snapshot, mut folds: Vec<Fold>) -> Self {
        folds.sort_by_key(|fold| (fold.bytes.start, fold.bytes.end));
        folds.dedup();
        let lines = folds.iter().map(|fold| fold.lines.clone());
        let shown = ShownLines::new(snapshot.line_count(), lines);
        Self {
            snapshot,
            folds,
            shown,
        }
    }

    /// The folds in `text`, those an edit since touched gone; the others' lines move by
    /// the lines the edit added, not looked up one by one.
    fn followed(&self, text: &Snapshot) -> Option<Self> {
        let edit = text.edit_from(&self.snapshot)?;
        let lines = |text: &Snapshot, end| text.line_of(end) - text.line_of(edit.start);
        let grown_lines =
            lines(text, edit.new_end) as isize - lines(&self.snapshot, edit.old_end) as isize;
        let grown = edit.new_end as isize - edit.old_end as isize;
        let shift = |range: &Range<usize>, by| {
            range.start.saturating_add_signed(by)..range.end.saturating_add_signed(by)
        };
        let folds = self.folds.iter().filter_map(|fold| {
            if edit.old_end < fold.bytes.start {
                Some(Fold {
                    bytes: shift(&fold.bytes, grown),
                    lines: shift(&fold.lines, grown_lines),
                })
            } else {
                (edit.start > fold.bytes.end).then(|| fold.clone())
            }
        });
        Some(Self::new(text.clone(), folds.collect()))
    }
}

/// A buffer's lines as windows show them: all but those folds hide. Windows count their
/// rows in the lines shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShownLines {
    line_count: usize,
    /// Disjoint, in order, each with the lines hidden before it.
    hidden: Vec<(Range<usize>, usize)>,
}

impl ShownLines {
    /// Of `line_count` lines with `hidden` ones, in order of their starts, each after a
    /// shown line.
    pub fn new(line_count: usize, hidden: impl IntoIterator<Item = Range<usize>>) -> Self {
        let mut merged: Vec<Range<usize>> = Vec::new();
        for range in hidden {
            let range = range.start.max(1)..range.end.min(line_count);
            if range.is_empty() {
                continue;
            }
            match merged.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => merged.push(range),
            }
        }
        let mut before = 0;
        let hidden = merged
            .into_iter()
            .map(|range| {
                let this = before;
                before += range.len();
                (range, this)
            })
            .collect();
        Self { line_count, hidden }
    }

    /// All `line_count` lines, none hidden.
    pub fn all(line_count: usize) -> Self {
        Self::new(line_count, [])
    }

    pub fn rows(&self) -> usize {
        let hidden = (self.hidden.last()).map_or(0, |(range, before)| before + range.len());
        (self.line_count - hidden).max(1)
    }

    pub fn is_hidden(&self, line: usize) -> bool {
        let index = self.hidden.partition_point(|(range, _)| range.end <= line);
        (self.hidden.get(index)).is_some_and(|(range, _)| range.start <= line)
    }

    /// The row `line` is shown on; a hidden line's is its fold's header's.
    pub fn row_of(&self, line: usize) -> usize {
        let line = line.min(self.line_count.saturating_sub(1));
        let index = self
            .hidden
            .partition_point(|(range, _)| range.start <= line);
        let Some((range, before)) = index.checked_sub(1).map(|index| &self.hidden[index]) else {
            return line;
        };
        if line < range.end {
            range.start - 1 - before
        } else {
            line - before - range.len()
        }
    }

    /// The line shown on `row`, the last one past the last row.
    pub fn line_at(&self, row: usize) -> usize {
        let row = row.min(self.rows() - 1);
        // A range's start less the lines hidden before it is the row of the line after it.
        let index = (self.hidden).partition_point(|(range, before)| range.start - before <= row);
        match index.checked_sub(1).map(|index| &self.hidden[index]) {
            Some((range, before)) => row + before + range.len(),
            None => row,
        }
    }

    /// The shown line `rows` rows from `line`, which may be hidden, within the lines.
    pub fn line_by(&self, line: usize, rows: isize) -> usize {
        self.line_at(self.row_of(line).saturating_add_signed(rows))
    }

    /// The lines a fold hides after `line`, its header; none when it heads no fold.
    pub fn hidden_after(&self, line: usize) -> Option<usize> {
        let index = self
            .hidden
            .partition_point(|(range, _)| range.start <= line);
        (self.hidden.get(index))
            .filter(|(range, _)| range.start == line + 1)
            .map(|(range, _)| range.len())
    }
}

impl Buffer {
    /// The lines as windows show them, with folds' lines hidden.
    pub fn shown_lines(&self) -> ShownLines {
        let line_count = self.text().line_count();
        let Some(folds) = self.local::<Folds>() else {
            return ShownLines::all(line_count);
        };
        match folds.followed(&self.text().snapshot()) {
            Some(followed) => followed.shown,
            None => folds.shown.clone(),
        }
    }

    /// Hides each of `folds`, ranges of whole lines after their header, which stays shown.
    pub fn fold(&mut self, folds: impl IntoIterator<Item = Range<usize>>) {
        let text = self.text().snapshot();
        let mut current = self.current_folds();
        current.extend(folds.into_iter().filter_map(|lines| {
            let last = lines.end.checked_sub(1)?;
            let start = text.line_start(lines.start).filter(|_| lines.start > 0)?;
            let end = text.line_range(last).filter(|_| lines.start <= last)?.end;
            Some(Fold {
                bytes: start..end,
                lines,
            })
        }));
        self.set_folds(current);
    }

    /// The lines each fold hides, folds within folds too, for `fold` to hide again.
    pub fn folded_lines(&self) -> Vec<Range<usize>> {
        (self.current_folds().into_iter())
            .map(|fold| fold.lines)
            .collect()
    }

    /// Shows the lines the folds `line` heads hide; false when it heads none.
    pub fn unfold_at(&mut self, line: usize) -> bool {
        !(self.unfold_where(|fold| fold.lines.start == line + 1)).is_empty()
    }

    /// Shows the lines the folds around `offset` hide, as hideshow does when point goes
    /// in; returns those lines, for `fold` to hide again.
    pub fn reveal(&mut self, offset: usize) -> Vec<Range<usize>> {
        let unfolded =
            self.unfold_where(|fold| fold.bytes.start <= offset && offset <= fold.bytes.end);
        unfolded.into_iter().map(|fold| fold.lines).collect()
    }

    pub fn unfold_all(&mut self) -> bool {
        let folded = self.local::<Folds>().is_some();
        self.remove_local::<Folds>();
        folded
    }

    /// Places the folds in the text as it is now, dropping those edits touched; after
    /// each edit, or, for those undo or a revert makes, after the command.
    pub(crate) fn follow_folds(&mut self) {
        let Some(folds) = self.local::<Folds>() else {
            return;
        };
        let followed = folds.followed(&self.text().snapshot());
        if let Some(followed) = followed {
            self.set_folds(followed.folds);
        }
    }

    /// Removes the folds `unfolds` picks, and returns them.
    fn unfold_where(&mut self, unfolds: impl Fn(&Fold) -> bool) -> Vec<Fold> {
        let (unfolded, kept): (Vec<_>, Vec<_>) =
            self.current_folds().into_iter().partition(unfolds);
        if !unfolded.is_empty() {
            self.set_folds(kept);
        }
        unfolded
    }

    fn current_folds(&self) -> Vec<Fold> {
        let Some(folds) = self.local::<Folds>() else {
            return Vec::new();
        };
        match folds.followed(&self.text().snapshot()) {
            Some(followed) => followed.folds,
            None => folds.folds.clone(),
        }
    }

    fn set_folds(&mut self, folds: Vec<Fold>) {
        if folds.is_empty() {
            self.remove_local::<Folds>();
        } else {
            let snapshot = self.text().snapshot();
            self.set_local(Folds::new(snapshot, folds));
        }
    }
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use remacs_text::Text;

    use super::*;

    #[test]
    fn rows_skip_the_hidden_lines() {
        // Lines 2–4 and 7 hidden of 10.
        let shown = ShownLines::new(10, [2..5, 3..4, 7..8]);
        assert_eq!(shown.rows(), 6);
        let lines: Vec<_> = (0..shown.rows()).map(|row| shown.line_at(row)).collect();
        assert_eq!(lines, [0, 1, 5, 6, 8, 9]);
        for (row, &line) in lines.iter().enumerate() {
            assert_eq!(shown.row_of(line), row);
        }
        assert_eq!(shown.row_of(3), 1, "a hidden line is on its header's row");
        assert_eq!(shown.row_of(7), 3);
        assert_eq!(shown.line_at(100), 9);
        assert_eq!(shown.line_by(1, 1), 5);
        assert_eq!(shown.line_by(5, -1), 1);
        assert_eq!(shown.hidden_after(1), Some(3));
        assert_eq!(shown.hidden_after(6), Some(1));
        assert_eq!(shown.hidden_after(0), None);
        assert!(shown.is_hidden(4) && !shown.is_hidden(5));
    }

    #[test]
    fn folds_follow_edits_and_go_where_an_edit_touches_them() {
        let mut buffer = Buffer::new("t", Text::from("a {\n b\n c\n}\nd\n"), "Fundamental");
        buffer.fold([1..3]);
        assert_eq!(buffer.shown_lines(), ShownLines::new(6, [1..3]));
        buffer.set_point(0);
        buffer.insert("x\n");
        let moved = ShownLines::new(7, [2..4]);
        assert_eq!(buffer.shown_lines(), moved, "an edit above moves it");
        buffer.set_point(buffer.text().len());
        buffer.insert("e");
        buffer.follow_folds();
        assert_eq!(buffer.shown_lines(), moved, "an edit below leaves it");
        let inside = buffer.text().line_start(3).unwrap();
        buffer.set_point(inside);
        buffer.insert("y");
        assert_eq!(
            buffer.shown_lines(),
            ShownLines::all(7),
            "an edit inside unfolds it"
        );
    }

    #[test]
    fn unfolding_takes_the_folds_a_line_heads_or_point_is_in() {
        let mut buffer = Buffer::new("t", Text::from("a\nb\nc\nd\ne\n"), "Fundamental");
        buffer.fold([1..4, 2..3]);
        assert!(buffer.unfold_at(0));
        assert_eq!(
            buffer.shown_lines(),
            ShownLines::new(6, [2..3]),
            "the inner one stays"
        );
        assert!(!buffer.unfold_at(4));
        let offset = buffer.text().line_start(2).unwrap();
        assert_eq!(buffer.reveal(offset), [2..3]);
        assert_eq!(buffer.shown_lines(), ShownLines::all(6));
        buffer.fold([1..2]);
        assert!(buffer.unfold_all());
        assert!(!buffer.unfold_all());
    }
}
