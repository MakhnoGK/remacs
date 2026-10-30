use std::ops::Range;

use remacs_core::{Buffer, CommandResult, Editor};

use crate::pairs::{Pair, mode_pairs, pairs_near};

pub const EXPAND_REGION: &str = "expand-region";
pub const CONTRACT_REGION: &str = "contract-region";

/// What `expand-region` selects in modes without pairs of their own.
const DEFAULT_PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"')];

/// The selections `expand-region` grew from, the latest last, for `contract-region` to go
/// back through; kept only across a run of the two commands.
#[derive(Default)]
struct ExpandedFrom(Vec<Selection>);

#[derive(Clone, Copy)]
struct Selection {
    point: usize,
    mark: Option<usize>,
    mark_active: bool,
}

impl Selection {
    fn of(buffer: &Buffer) -> Self {
        Self { point: buffer.point(), mark: buffer.mark(), mark_active: buffer.is_mark_active() }
    }

    fn restore(self, buffer: &mut Buffer) {
        if let Some(mark) = self.mark {
            buffer.set_mark(mark);
        }
        buffer.set_point(self.point);
        if self.mark_active {
            buffer.activate_mark();
        } else {
            buffer.deactivate_mark();
        }
    }
}

/// Selects inside the innermost brackets or quotes around the region or point, then the
/// pair with them, then inside the next pair out.
pub fn expand_region(editor: &mut Editor) -> CommandResult {
    let pairs = match mode_pairs(editor) {
        [] => DEFAULT_PAIRS,
        pairs => pairs,
    };
    let resizing = resizing(editor);
    let buffer = editor.buffer_mut();
    let point = buffer.point();
    let selected = buffer.region().unwrap_or(point..point);
    let mut around: Vec<Pair> = pairs_near(buffer, pairs, &selected)
        .into_iter()
        .filter(|pair| surrounds(&pair.outer, &selected))
        .collect();
    around.sort_by_key(|pair| pair.outer.len());
    let expanded = around
        .into_iter()
        .flat_map(|pair| [pair.inner, pair.outer])
        .find(|range| contains(range, &selected) && *range != selected)
        .ok_or("No brackets or quotes around")?;

    if !resizing || buffer.local::<ExpandedFrom>().is_none() {
        buffer.set_local(ExpandedFrom::default());
    }
    let earlier = Selection::of(buffer);
    if let Some(history) = buffer.local_mut::<ExpandedFrom>() {
        history.0.push(earlier);
    }
    buffer.set_mark(expanded.start);
    buffer.set_point(expanded.end);
    buffer.activate_mark();
    Ok(())
}

/// Undoes the last `expand-region`, back to the selection or point it grew from.
pub fn contract_region(editor: &mut Editor) -> CommandResult {
    let resizing = resizing(editor);
    let buffer = editor.buffer_mut();
    let earlier = buffer
        .local_mut::<ExpandedFrom>()
        .filter(|_| resizing)
        .and_then(|history| history.0.pop())
        .ok_or("No expanded region to contract")?;
    earlier.restore(buffer);
    Ok(())
}

fn resizing(editor: &Editor) -> bool {
    matches!(editor.last_command(), Some(EXPAND_REGION | CONTRACT_REGION))
}

/// Whether `outer` holds `selected`, point strictly inside when nothing is selected, so
/// point just before or after a pair does not select it.
fn surrounds(outer: &Range<usize>, selected: &Range<usize>) -> bool {
    contains(outer, selected)
        && (!selected.is_empty() || (outer.start < selected.start && selected.end < outer.end))
}

fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
