//! Moving over expressions and in and out of lists as the syntax tree has them, like Emacs
//! `forward-sexp` and `up-list` in tree-sitter modes. The expressions point is between are
//! the children of the innermost node it is inside of; a list is a node between brackets.

use remacs_core::{CommandResult, Editor};
use tree_sitter::Node;

use crate::buffer_syntax::tree_at;

pub const FORWARD_SEXP: &str = "forward-sexp";
pub const BACKWARD_SEXP: &str = "backward-sexp";
pub const BACKWARD_UP_LIST: &str = "backward-up-list";
pub const DOWN_LIST: &str = "down-list";
pub const FORWARD_LIST: &str = "forward-list";
pub const BACKWARD_LIST: &str = "backward-list";

const BRACKETS: &[(&str, &str)] = &[("(", ")"), ("[", "]"), ("{", "}")];

/// Moves over the next expression.
pub(crate) fn forward_sexp(editor: &mut Editor) -> CommandResult {
    move_by(
        editor,
        |node, point| {
            if node.child_count() == 0 {
                return Some(node.end_byte());
            }
            inside(node)
                .find(|child| child.end_byte() > point)
                .map(|child| child.end_byte())
        },
        "No next expression",
    )
}

/// Moves back over the previous expression.
pub(crate) fn backward_sexp(editor: &mut Editor) -> CommandResult {
    move_by(
        editor,
        |node, point| {
            if node.child_count() == 0 {
                return Some(node.start_byte());
            }
            inside(node)
                .filter(|child| child.start_byte() < point)
                .last()
                .map(|child| child.start_byte())
        },
        "No previous expression",
    )
}

/// Moves to the opening bracket of the list point is in.
pub(crate) fn backward_up_list(editor: &mut Editor) -> CommandResult {
    move_by(
        editor,
        |node, point| {
            ancestors(node)
                .find(|node| is_list(node) && node.start_byte() < point)
                .map(|list| list.start_byte())
        },
        "Not in a list",
    )
}

/// Moves into the next list, just after its opening bracket.
pub(crate) fn down_list(editor: &mut Editor) -> CommandResult {
    move_by(
        editor,
        |node, point| {
            let after = expressions(node).filter(|child| child.start_byte() >= point);
            first_list(after).map(|list| {
                list.child(0)
                    .map_or(list.start_byte(), |open| open.end_byte())
            })
        },
        "No list ahead",
    )
}

/// Moves over the next list, skipping what is not in one.
pub(crate) fn forward_list(editor: &mut Editor) -> CommandResult {
    move_by(
        editor,
        |node, point| {
            first_list(expressions(node).filter(|child| child.start_byte() >= point))
                .map(|list| list.end_byte())
        },
        "No list ahead",
    )
}

/// Moves back over the previous list, skipping what is not in one.
pub(crate) fn backward_list(editor: &mut Editor) -> CommandResult {
    move_by(
        editor,
        |node, point| {
            let before: Vec<_> = expressions(node)
                .filter(|child| child.end_byte() <= point)
                .collect();
            last_list(before.into_iter().rev()).map(|list| list.start_byte())
        },
        "No list behind",
    )
}

/// Moves point to where `target` finds from the innermost node point is inside of, else
/// fails with `nowhere`.
fn move_by(
    editor: &mut Editor,
    target: impl Fn(Node, usize) -> Option<usize>,
    nowhere: &str,
) -> CommandResult {
    let buffer = editor.buffer();
    let point = buffer.point();
    let tree = tree_at(buffer, point..point).ok_or("No syntax tree for this buffer")?;
    let offset = target(innermost(tree.root_node(), point), point).ok_or(nowhere)?;
    editor.buffer_mut().set_point(offset);
    Ok(())
}

/// The innermost node that `offset` is strictly inside of, or the root.
fn innermost(root: Node, offset: usize) -> Node {
    let mut node = root;
    while let Some(child) =
        expressions(node).find(|child| child.start_byte() < offset && offset < child.end_byte())
    {
        node = child;
    }
    node
}

/// The children of `node` that hold text: a parser fills in what is missing with empty ones.
fn expressions<'tree>(node: Node<'tree>) -> impl Iterator<Item = Node<'tree>> {
    (0..node.child_count())
        .filter_map(move |index| node.child(index))
        .filter(|child| child.start_byte() < child.end_byte())
}

/// The expressions of `node` but for its brackets, which motion stays between.
fn inside<'tree>(node: Node<'tree>) -> impl Iterator<Item = Node<'tree>> {
    let brackets = is_list(&node).then(|| {
        let last = node.child_count() - 1;
        (node.child(0), node.child(last))
    });
    expressions(node).filter(move |child| {
        brackets.is_none_or(|(open, close)| Some(*child) != open && Some(*child) != close)
    })
}

fn ancestors(node: Node) -> impl Iterator<Item = Node> {
    std::iter::successors(Some(node), Node::parent)
}

/// Whether the node is between brackets that open and close it.
fn is_list(node: &Node) -> bool {
    let count = node.child_count();
    let (Some(first), Some(last)) = (
        node.child(0),
        count.checked_sub(1).and_then(|last| node.child(last)),
    ) else {
        return false;
    };
    count >= 2
        && !first.is_named()
        && BRACKETS
            .iter()
            .any(|&(open, close)| first.kind() == open && last.kind() == close)
}

/// The first list among `nodes` and what they hold, the outermost one where lists nest.
fn first_list<'tree>(nodes: impl Iterator<Item = Node<'tree>>) -> Option<Node<'tree>> {
    nodes.into_iter().find_map(|node| {
        if is_list(&node) {
            Some(node)
        } else {
            first_list(expressions(node))
        }
    })
}

/// Like [`first_list`] over `nodes` given last first.
fn last_list<'tree>(nodes: impl Iterator<Item = Node<'tree>>) -> Option<Node<'tree>> {
    nodes.into_iter().find_map(|node| {
        if is_list(&node) {
            Some(node)
        } else {
            let children: Vec<_> = expressions(node).collect();
            last_list(children.into_iter().rev())
        }
    })
}
