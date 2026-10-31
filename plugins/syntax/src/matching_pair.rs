//! The bracket or tag at point highlighted with the one it pairs with, like Emacs
//! `show-paren-mode`. Read from the tree, so brackets in strings and comments are left out.

use std::iter;
use std::ops::Range;

use remacs_core::{Buffer, Face, Span};
use tree_sitter::Node;

use crate::buffer_syntax::tree_at;

const BRACKETS: &[(&str, &str)] = &[("(", ")"), ("[", "]"), ("{", "}"), ("<", ">")];
/// Opening and closing tags of HTML, XML and JSX elements.
const TAGS: &[(&str, &str)] =
    &[("start_tag", "end_tag"), ("STag", "ETag"), ("jsx_opening_element", "jsx_closing_element")];
/// A tag's name, where the grammar has no `name` field for it.
const TAG_NAMES: &[&str] = &["tag_name", "Name"];

pub(crate) fn highlight(buffer: &Buffer, range: Range<usize>) -> Vec<Span> {
    let Some(mut pair) = matching_pair(buffer) else {
        return Vec::new();
    };
    pair.sort_by_key(|part| part.start);
    pair.into_iter()
        .filter(|part| part.start < range.end && range.start < part.end)
        .map(|part| Span { range: part, face: Face::MatchingPair })
        .collect()
}

/// The ranges of what pairs up at point: the character after point, else the one before.
fn matching_pair(buffer: &Buffer) -> Option<[Range<usize>; 2]> {
    let point = buffer.point();
    [Some(point), point.checked_sub(1)]
        .into_iter()
        .flatten()
        .filter(|offset| *offset < buffer.text().len())
        .find_map(|offset| {
            let root = tree_at(buffer, offset..offset + 1)?.root_node();
            let node = root.descendant_for_byte_range(offset, offset + 1)?;
            matching_tag(node, offset).or_else(|| matching_bracket(node, offset))
        })
}

fn matching_bracket<'tree>(node: Node<'tree>, offset: usize) -> Option<[Range<usize>; 2]> {
    if node.byte_range() != (offset..offset + 1) {
        return None;
    }
    let kind = node.kind();
    let &(open, close) = BRACKETS.iter().find(|(open, close)| kind == *open || kind == *close)?;
    let forward = kind == open;
    let other = if forward { close } else { open };
    let step = move |sibling: &Node<'tree>| {
        if forward { sibling.next_sibling() } else { sibling.prev_sibling() }
    };
    let mut depth = 0usize;
    for sibling in iter::successors(step(&node), step) {
        if sibling.kind() == kind {
            depth += 1;
        } else if sibling.kind() == other {
            if depth == 0 {
                return (!sibling.is_missing()).then(|| [node.byte_range(), sibling.byte_range()]);
            }
            depth -= 1;
        }
    }
    None
}

/// The names of a tag and its partner, when `node` at `offset` is the tag's name or its
/// own punctuation, not an attribute.
fn matching_tag(node: Node, offset: usize) -> Option<[Range<usize>; 2]> {
    let tag = iter::successors(Some(node), Node::parent)
        .find(|ancestor| tag_kinds(*ancestor).is_some())?;
    let name = tag_name(tag);
    let on_tag = node == tag
        || (node.parent() == Some(tag) && !node.is_named())
        || name.is_some_and(|name| name.byte_range().contains(&offset));
    if !on_tag {
        return None;
    }
    let (open, close) = tag_kinds(tag)?;
    let other = if tag.kind() == open { close } else { open };
    let element = tag.parent()?;
    let mut cursor = element.walk();
    let partner =
        element.children(&mut cursor).find(|child| child.kind() == other && !child.is_missing())?;
    Some([name_range(tag), name_range(partner)])
}

fn tag_kinds(node: Node) -> Option<(&'static str, &'static str)> {
    TAGS.iter().find(|(open, close)| node.kind() == *open || node.kind() == *close).copied()
}

fn tag_name(tag: Node) -> Option<Node> {
    tag.child_by_field_name("name").or_else(|| {
        let mut cursor = tag.walk();
        tag.named_children(&mut cursor).find(|child| TAG_NAMES.contains(&child.kind()))
    })
}

/// A fragment's `<>` and `</>` have no name: the whole tag.
fn name_range(tag: Node) -> Range<usize> {
    tag_name(tag).unwrap_or(tag).byte_range()
}
