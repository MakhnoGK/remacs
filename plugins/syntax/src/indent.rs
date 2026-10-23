//! Indentation read from the syntax tree, as each mode's indent query captures it (ADR 0084).

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use remacs_core::{Buffer, Indentation};
use remacs_text::Snapshot;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, Tree};

use crate::highlighter::Chunks;
use crate::{Grammar, Indents, buffer_syntax, parser_thread};

/// Text the parser thread has not parsed yet is parsed here only up to this size; a larger
/// one waits for it.
const MAX_FIRST_PARSE: usize = 1 << 20;
/// Bounds the scans of a line's blanks and of the blank lines above it.
const MAX_SCAN: usize = 64 << 10;
const MAX_BLANK_LINES: usize = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Begin,
    /// Its lines after its first line up with its start.
    Align,
    /// Ends or continues a begin node at its level: `}`, `end`, `else`.
    Close,
    Open,
}

fn role(capture_name: &str) -> Option<Role> {
    match capture_name {
        "indent.begin" => Some(Role::Begin),
        "indent.align" => Some(Role::Align),
        "indent.end" | "indent.branch" => Some(Role::Close),
        "indent.open" => Some(Role::Open),
        _ => None,
    }
}

/// A compiled indent query and the role of each of its captures.
struct IndentQuery {
    query: Query,
    roles: Vec<Option<Role>>,
}

impl IndentQuery {
    pub fn new(language: &Language, sources: &[&str]) -> Result<Self, String> {
        let query = Query::new(language, &sources.join("\n")).map_err(|error| error.to_string())?;
        let names = query.capture_names();
        if let Some(unknown) = names
            .iter()
            .find(|name| !name.starts_with('_') && role(name).is_none())
        {
            return Err(format!("unknown capture @{unknown}"));
        }
        let roles = names.iter().map(|name| role(name)).collect();
        Ok(Self { query, roles })
    }

    /// The nodes captured around `range`, with their roles.
    fn roles_at(&self, tree: &Tree, text: &Snapshot, range: Range<usize>) -> Roles {
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range);
        let mut captures = cursor.captures(&self.query, tree.root_node(), Chunks(text));
        let mut roles = Vec::new();
        while let Some((matched, index)) = captures.next() {
            let capture = matched.captures()[*index];
            if let Some(role) = self.roles[capture.index as usize] {
                roles.push((capture.node.id(), role));
            }
        }
        Roles(roles)
    }
}

struct Roles(Vec<(usize, Role)>);

impl Roles {
    fn has(&self, node: Node, role: Role) -> bool {
        self.0.contains(&(node.id(), role))
    }
}

/// Answers the editor's indent probe: keeps a parser for text the parser thread has not
/// caught up with, its last tree, and each mode's compiled query.
pub(crate) struct Indenter {
    parser: Parser,
    parsed: Option<(&'static str, Snapshot, Tree)>,
    queries: HashMap<&'static str, Option<Rc<IndentQuery>>>,
}

impl Indenter {
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
            parsed: None,
            queries: HashMap::new(),
        }
    }

    pub fn indentation(
        &mut self,
        indents: &'static [Indents],
        buffer: &Buffer,
        line: usize,
    ) -> Option<Indentation> {
        let text = buffer.text().snapshot();
        let range = text.line_range(line)?;
        if buffer_syntax::in_injected_code(buffer, range.start) {
            return None;
        }
        let (grammar, tree, parsed) = buffer_syntax::tree_for(buffer, &text)?;
        let indents = indents
            .iter()
            .find(|indents| indents.mode == grammar.mode)?;
        if indents.only_blank_lines && first_code(&text, range).is_some() {
            return None;
        }
        let query = self.query(grammar, indents)?;
        let tree = match (tree, parsed) {
            (Some(tree), true) => tree,
            (tree, _) => self.parse(grammar, &text, tree)?,
        };
        indentation(&query, &tree, &text, line)
    }

    pub fn forget_parse(&mut self) {
        self.parsed = None;
    }

    fn query(&mut self, grammar: &Grammar, indents: &Indents) -> Option<Rc<IndentQuery>> {
        self.queries
            .entry(grammar.mode)
            .or_insert_with(|| {
                IndentQuery::new(&Language::new(grammar.language), indents.queries)
                    .inspect_err(|error| eprintln!("remacs: {} indents: {error}", grammar.mode))
                    .ok()
                    .map(Rc::new)
            })
            .clone()
    }

    /// `text` parsed again from `old`, its tree edited to follow it.
    fn parse(&mut self, grammar: &Grammar, text: &Snapshot, old: Option<Tree>) -> Option<Tree> {
        if let Some((mode, parsed, tree)) = &self.parsed
            && *mode == grammar.mode
            && parsed.is_same(text)
        {
            return Some(tree.clone());
        }
        if old.is_none() && text.len() > MAX_FIRST_PARSE {
            return None;
        }
        self.parser
            .set_language(&Language::new(grammar.language))
            .ok()?;
        let tree = parser_thread::parse(&mut self.parser, text, old.as_ref())?;
        self.parsed = Some((grammar.mode, text.clone(), tree.clone()));
        Some(tree)
    }
}

/// Where `line` of `text` takes its indentation from, read from `tree`; `None` where the
/// tree is broken around it, which the brackets then tell better.
fn indentation(
    query: &IndentQuery,
    tree: &Tree,
    text: &Snapshot,
    line: usize,
) -> Option<Indentation> {
    let range = text.line_range(line)?;
    let Some(start) = first_code(text, range.clone()) else {
        return blank_line_indentation(query, tree, text, line, range.start);
    };
    let leaf = tree
        .root_node()
        .descendant_for_byte_range(start, start + 1)?;
    if leaf.start_position().row < line {
        // Within a string or comment begun above: left as written.
        return Some(at_line(line, false));
    }
    let roles = query.roles_at(tree, text, start..start + 1);
    let closes = ancestors(leaf)
        .take_while(|node| node.start_byte() == start)
        .any(|node| roles.has(node, Role::Close));
    let outer = ancestors(leaf)
        .take_while(|node| !node.is_error())
        .find(|node| {
            node.start_position().row < line
                && (roles.has(*node, Role::Begin) || roles.has(*node, Role::Align))
        });
    if outer.is_none() && ancestors(leaf).any(|node| node.is_error()) {
        return None;
    }
    Some(outer.map_or_else(Indentation::default, |outer| {
        within(text, &roles, outer, !closes)
    }))
}

/// A blank line, as one breaking a line leaves: deeper after an opening token, deeper than
/// a begin node it is within, else at the level of the line above.
fn blank_line_indentation(
    query: &IndentQuery,
    tree: &Tree,
    text: &Snapshot,
    line: usize,
    start: usize,
) -> Option<Indentation> {
    let Some((above, end)) = last_code_above(text, line) else {
        return Some(Indentation::default());
    };
    let root = tree.root_node();
    let roles = query.roles_at(tree, text, end - 1..start + 1);
    let last = root.descendant_for_byte_range(end - 1, end)?;
    if roles.has(last, Role::Open) {
        return Some(at_line(above, true));
    }
    // Code being typed, like an unclosed call: the brackets tell.
    if ancestors(last).any(|node| node.is_error()) {
        return None;
    }
    for node in ancestors(root.descendant_for_byte_range(start, start)?) {
        if node.is_error() {
            return None;
        }
        let around = node.start_position().row < line && node.end_position().row > line;
        if around && (roles.has(node, Role::Begin) || roles.has(node, Role::Align)) {
            return Some(within(text, &roles, node, true));
        }
    }
    Some(at_line(above, false))
}

fn at_line(anchor: usize, deeper: bool) -> Indentation {
    Indentation {
        anchor: Some(anchor),
        deeper,
        align: 0,
    }
}

/// A line within `outer`: lined up with it if it aligns, else a level deeper than its
/// first line unless `deeper` is false.
fn within(text: &Snapshot, roles: &Roles, outer: Node, deeper: bool) -> Indentation {
    let row = outer.start_position().row;
    if !roles.has(outer, Role::Align) {
        return at_line(row, deeper);
    }
    let line_start = text.line_start(row).unwrap_or(0);
    let code = first_code(text, line_start..outer.start_byte()).unwrap_or(outer.start_byte());
    let before = text.bytes_in(code..outer.start_byte());
    Indentation {
        align: String::from_utf8_lossy(&before).chars().count(),
        ..at_line(row, false)
    }
}

fn ancestors(node: Node) -> impl Iterator<Item = Node> {
    std::iter::successors(Some(node), Node::parent)
}

/// The first byte of `line` that is not a blank; `None` for a blank line.
fn first_code(text: &Snapshot, line: Range<usize>) -> Option<usize> {
    let head = text.bytes_in(line.start..line.end.min(line.start + MAX_SCAN));
    let blanks = head.iter().position(|byte| !byte.is_ascii_whitespace())?;
    Some(line.start + blanks)
}

/// The nearest line above `line` that is not blank, and the end of its last token.
fn last_code_above(text: &Snapshot, line: usize) -> Option<(usize, usize)> {
    (line.saturating_sub(MAX_BLANK_LINES)..line)
        .rev()
        .find_map(|above| {
            let range = text.line_range(above)?;
            let start = range.start.max(range.end.saturating_sub(MAX_SCAN));
            let tail = text.bytes_in(start..range.end);
            let end = tail.iter().rposition(|byte| !byte.is_ascii_whitespace())?;
            Some((above, start + end + 1))
        })
}
