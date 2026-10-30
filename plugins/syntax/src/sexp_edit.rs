//! Editing expressions and lists as the syntax tree has them, like Emacs `kill-sexp` and
//! paredit's slurping and barfing. Separators like `,` and `;` are not expressions here,
//! so `(a, b, c)` barfs to `(a, b), c`.

use std::ops::Range;

use remacs_core::{CommandResult, Editor};
use tree_sitter::Node;

use crate::buffer_syntax::{is_parsed, is_reparsed};
use crate::sexp::{find_at, inside, is_list, list_around, next_end, previous_start};

pub const TRANSPOSE_SEXPS: &str = "transpose-sexps";
pub const KILL_SEXP: &str = "kill-sexp";
pub const BACKWARD_KILL_SEXP: &str = "backward-kill-sexp";
pub const MARK_SEXP: &str = "mark-sexp";
pub const SLURP_FORWARD: &str = "slurp-forward";
pub const BARF_FORWARD: &str = "barf-forward";
pub const SLURP_BACKWARD: &str = "slurp-backward";
pub const BARF_BACKWARD: &str = "barf-backward";
pub const SPLICE_SEXP: &str = "splice-sexp";
pub const RAISE_SEXP: &str = "raise-sexp";

/// Swaps the expressions around point, leaving point after both.
pub(crate) fn transpose_sexps(editor: &mut Editor) -> CommandResult {
    ensure_editable(editor)?;
    let buffer = editor.buffer();
    let (before, after) = find_at(
        buffer,
        buffer.point(),
        |node, point| {
            let before = items(node).filter(|item| item.end_byte() <= point).last()?;
            let after = items(node).find(|item| item.start_byte() >= point)?;
            Some((before.byte_range(), after.byte_range()))
        },
        "Don't have two things to transpose",
    )?;
    let buffer = editor.buffer_mut();
    let (first, second) = (buffer.contents(before.clone()), buffer.contents(after.clone()));
    buffer.apply_edits(vec![(before, second), (after.clone(), first)]);
    buffer.set_point(after.end);
    Ok(())
}

pub(crate) fn kill_sexp(editor: &mut Editor) -> CommandResult {
    ensure_editable(editor)?;
    let point = editor.buffer().point();
    let end = find_at(editor.buffer(), point, next_end, "No next expression")?;
    editor.kill(point..end);
    Ok(())
}

pub(crate) fn backward_kill_sexp(editor: &mut Editor) -> CommandResult {
    ensure_editable(editor)?;
    let point = editor.buffer().point();
    let start = find_at(editor.buffer(), point, previous_start, "No previous expression")?;
    editor.kill(start..point);
    Ok(())
}

/// Sets the mark after the next expression; right after itself, after the one after the
/// mark.
pub(crate) fn mark_sexp(editor: &mut Editor) -> CommandResult {
    let repeated = editor.last_command() == Some(MARK_SEXP);
    let buffer = editor.buffer();
    let from = match buffer.mark() {
        Some(mark) if repeated && buffer.is_mark_active() => mark,
        _ => buffer.point(),
    };
    let end = find_at(buffer, from, next_end, "No next expression")?;
    let buffer = editor.buffer_mut();
    if from == buffer.point() {
        buffer.push_mark();
    }
    buffer.set_mark(end);
    buffer.activate_mark();
    Ok(())
}

/// Moves the closing bracket of the list point is in after the expression that follows it.
pub(crate) fn slurp_forward(editor: &mut Editor) -> CommandResult {
    move_bracket(
        editor,
        |list| Some((close(list), neighbor(list, true)?.end_byte())),
        "Nothing to slurp",
    )
}

/// Moves the opening bracket of the list point is in before the expression before it.
pub(crate) fn slurp_backward(editor: &mut Editor) -> CommandResult {
    move_bracket(
        editor,
        |list| Some((open(list), neighbor(list, false)?.start_byte())),
        "Nothing to slurp",
    )
}

/// Moves the closing bracket of the list point is in before its last expression.
pub(crate) fn barf_forward(editor: &mut Editor) -> CommandResult {
    move_bracket(
        editor,
        |list| {
            let items: Vec<_> = items(list).collect();
            items.last()?;
            let to =
                items.len().checked_sub(2).map_or(open(list).end, |index| items[index].end_byte());
            Some((close(list), to))
        },
        "Nothing to barf",
    )
}

/// Moves the opening bracket of the list point is in after its first expression.
pub(crate) fn barf_backward(editor: &mut Editor) -> CommandResult {
    move_bracket(
        editor,
        |list| {
            let mut items = items(list);
            items.next()?;
            let to = items.next().map_or(close(list).start, |item| item.start_byte());
            Some((open(list), to))
        },
        "Nothing to barf",
    )
}

/// Deletes the brackets of the list point is in.
pub(crate) fn splice_sexp(editor: &mut Editor) -> CommandResult {
    ensure_editable(editor)?;
    let buffer = editor.buffer();
    let (open, close) = find_at(
        buffer,
        buffer.point(),
        |node, point| list_around(node, point).map(|list| (open(list), close(list))),
        "Not in a list",
    )?;
    editor.buffer_mut().apply_edits(vec![(open, String::new()), (close, String::new())]);
    Ok(())
}

/// Replaces the list point is in, with what it closes, like a call with its arguments,
/// by the expression at or after point.
pub(crate) fn raise_sexp(editor: &mut Editor) -> CommandResult {
    ensure_editable(editor)?;
    let buffer = editor.buffer();
    let point = buffer.point();
    let (outer, raised) = find_at(
        buffer,
        point,
        |node, point| {
            let list = list_around(node, point)?;
            let raised = items(list).find(|item| item.end_byte() > point)?;
            Some((with_head(list).byte_range(), raised.byte_range()))
        },
        "Nothing to raise",
    )?;
    let buffer = editor.buffer_mut();
    let text = buffer.contents(raised.clone());
    let into = point.saturating_sub(raised.start).min(text.len());
    buffer.apply_edits(vec![(outer.clone(), text)]);
    buffer.set_point(outer.start + into);
    Ok(())
}

/// Moves a bracket of the list point is in to where `target` finds from the list.
fn move_bracket(
    editor: &mut Editor,
    target: impl Fn(Node) -> Option<(Range<usize>, usize)>,
    nowhere: &str,
) -> CommandResult {
    ensure_editable(editor)?;
    let buffer = editor.buffer();
    let (bracket, to) =
        find_at(buffer, buffer.point(), |node, point| target(list_around(node, point)?), nowhere)?;
    let buffer = editor.buffer_mut();
    let text = buffer.contents(bracket.clone());
    buffer.apply_edits(vec![(bracket, String::new()), (to..to, text)]);
    Ok(())
}

/// Fails in a read-only buffer, and until its tree is parsed from its text again: one only
/// moved along with the last edit may no longer hold its expressions.
fn ensure_editable(editor: &Editor) -> CommandResult {
    let buffer = editor.buffer();
    buffer.ensure_writable()?;
    if is_parsed(buffer) && !is_reparsed(buffer) {
        return Err("Still parsing the last change".into());
    }
    Ok(())
}

/// The expressions of `node` but for its brackets and separators.
fn items<'tree>(node: Node<'tree>) -> impl Iterator<Item = Node<'tree>> {
    inside(node).filter(Node::is_named)
}

fn open(list: Node) -> Range<usize> {
    list.child(0).map_or(0..0, |open| open.byte_range())
}

fn close(list: Node) -> Range<usize> {
    let last = list.child_count() - 1;
    list.child(last).map_or(0..0, |close| close.byte_range())
}

/// The expression after `list`, or before it, from the nodes that end, or start, with it,
/// so as not to go past a separator after it.
fn neighbor(list: Node, forward: bool) -> Option<Node> {
    let edge = |node: &Node| {
        if forward { node.end_byte() } else { node.start_byte() }
    };
    let mut node = list;
    loop {
        let sibling = if forward { node.next_named_sibling() } else { node.prev_named_sibling() };
        if let Some(sibling) = sibling {
            return Some(sibling);
        }
        node = node.parent().filter(|parent| edge(parent) == edge(&list))?;
    }
}

/// `list` with the nodes it ends, up to the list they are in, like a call with its
/// arguments.
fn with_head(list: Node) -> Node {
    let mut node = list;
    while let Some(parent) = node.parent().filter(|parent| {
        parent.parent().is_some() && parent.end_byte() == list.end_byte() && !is_list(parent)
    }) {
        node = parent;
    }
    node
}
