//! Indentation read from the syntax tree, as each mode's indent query captures it (ADR 0084).

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use remacs_core::{Block, Buffer, Indentation};
use remacs_text::Snapshot;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, Tree};

use crate::highlighter::Chunks;
use crate::{Grammar, Indents, buffer_syntax, parser_thread, sexp};

/// Text the parser thread has not parsed yet is parsed here only up to this size; a larger
/// one waits for it.
const MAX_FIRST_PARSE: usize = 1 << 20;
/// Bounds the scans of a line's blanks and of the blank lines above it.
const MAX_SCAN: usize = 64 << 10;
const MAX_BLANK_LINES: usize = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
        if let Some(unknown) =
            names.iter().find(|name| !name.starts_with('_') && role(name).is_none())
        {
            return Err(format!("unknown capture @{unknown}"));
        }
        let roles = names.iter().map(|name| role(name)).collect();
        Ok(Self { query, roles })
    }

    /// The nodes captured around `range`, with their roles.
    fn roles_at<'tree>(
        &self,
        tree: &'tree Tree,
        text: &Snapshot,
        range: Range<usize>,
    ) -> Roles<'tree> {
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range);
        let mut captures = cursor.captures(&self.query, tree.root_node(), Chunks(text));
        let mut roles = Vec::new();
        while let Some((matched, index)) = captures.next() {
            let capture = matched.captures()[*index];
            if let Some(role) = self.roles[capture.index as usize] {
                roles.push((capture.node, role));
            }
        }
        roles.into_iter().collect()
    }
}

struct Roles<'tree> {
    captured: Vec<(Node<'tree>, Role)>,
    /// By node id, so that looking up each node of a whole file stays linear.
    ids: HashSet<(usize, Role)>,
}

impl<'tree> FromIterator<(Node<'tree>, Role)> for Roles<'tree> {
    fn from_iter<T: IntoIterator<Item = (Node<'tree>, Role)>>(captured: T) -> Self {
        let captured: Vec<_> = captured.into_iter().collect();
        let ids = (captured.iter()).map(|(node, role)| (node.id(), *role)).collect();
        Self { captured, ids }
    }
}

impl Roles<'_> {
    fn has(&self, node: Node, role: Role) -> bool {
        self.ids.contains(&(node.id(), role))
    }
}

/// The code a line is in: the buffer's own or another language's, its tree edited to
/// follow the text, and whether that tree is a parse of the text itself.
struct Code {
    grammar: &'static Grammar,
    tree: Option<Tree>,
    parsed: bool,
    terminator: Option<u8>,
}

/// Answers the editor's indent probe: keeps a parser for text the parser thread has not
/// caught up with, its last tree, and each mode's compiled query.
pub(crate) struct Indenter {
    parser: Parser,
    /// The mode and start of the code parsed, the text and its tree.
    parsed: Option<(&'static str, usize, Snapshot, Tree)>,
    queries: HashMap<&'static str, Option<Rc<IndentQuery>>>,
}

impl Indenter {
    pub fn new() -> Self {
        Self { parser: Parser::new(), parsed: None, queries: HashMap::new() }
    }

    pub fn indentation(
        &mut self,
        indents: &'static [Indents],
        buffer: &Buffer,
        line: usize,
    ) -> Option<Indentation> {
        let text = buffer.text().snapshot();
        let range = text.line_range(line)?;
        // Code of another language, a script in a page, is indented by its own syntax; the
        // tag closing it, after its code, is the page's.
        let at = first_code(&text, range.clone())
            .map_or(range.start..range.start, |code| code..code + 1);
        let Some((layer, parsed)) = buffer_syntax::layer_for(buffer, &text, at) else {
            let (grammar, tree, parsed) = buffer_syntax::tree_for(buffer, &text)?;
            let code = Code { grammar, tree, parsed, terminator: None };
            return self.indentation_by(indents, code, &text, line);
        };
        let ranges = layer.tree.included_ranges();
        let in_code = |code: usize| {
            (ranges.iter()).any(|range| range.start_byte <= code && code < range.end_byte)
        };
        let code = Code {
            grammar: layer.grammar,
            tree: Some(layer.tree),
            parsed,
            terminator: layer.terminator,
        };
        let own = self.indentation_by(indents, code, &text, line)?;
        let anchored_in_code = (own.anchor)
            .and_then(|anchor| first_code(&text, text.line_range(anchor)?))
            .is_some_and(in_code);
        if anchored_in_code {
            return Some(own);
        }
        // Its top level lines up with its first line, as its author put it: a page's
        // scripts are a level deeper than their tag, a Vue component's are not.
        let first_line = (ranges.iter())
            .find_map(|range| first_code(&text, range.start_byte..range.end_byte))
            .map(|code| text.line_of(code))?;
        Some(at_line(first_line, false))
    }

    fn indentation_by(
        &mut self,
        indents: &'static [Indents],
        code: Code,
        text: &Snapshot,
        line: usize,
    ) -> Option<Indentation> {
        let indents = indents.iter().find(|indents| indents.mode == code.grammar.mode)?;
        if indents.only_blank_lines && first_code(text, text.line_range(line)?).is_some() {
            return None;
        }
        let query = self.query(code.grammar, indents)?;
        let tree = match (code.tree, code.parsed) {
            (Some(tree), true) => tree,
            (tree, _) => self.parse(code.grammar, text, tree, code.terminator)?,
        };
        indentation(&query, &tree, text, line)
    }

    pub fn forget_parse(&mut self) {
        self.parsed = None;
    }

    /// The blocks that touch `lines`: the indent query's begin nodes, or without one the
    /// lists between brackets, over more than one line; read from the tree as edited to
    /// follow the text, which is close enough to fold by.
    pub fn blocks(
        &mut self,
        indents: &'static [Indents],
        buffer: &Buffer,
        lines: Range<usize>,
    ) -> Option<Vec<Block>> {
        let text = buffer.text().snapshot();
        let (grammar, tree, _) = buffer_syntax::tree_for(buffer, &text)?;
        let tree = tree?;
        let start = text.line_start(lines.start)?;
        let end = (text.line_range(lines.end.max(lines.start + 1) - 1))
            .map_or(text.len(), |line| line.end);
        // shortcut: the code of other languages, a page's scripts, is read from its trees as
        // last parsed, a keystroke behind at most; edit them too if a fold misses by that.
        let injected = buffer_syntax::layers_in(buffer, &(start..end));
        let mut blocks = Vec::new();
        for (grammar, tree) in std::iter::once((grammar, &tree)).chain(injected) {
            let query = (indents.iter())
                .find(|indents| indents.mode == grammar.mode)
                .and_then(|indents| self.query(grammar, indents));
            let roles = query.map(|query| query.roles_at(tree, &text, start..end));
            let candidates: Vec<Node> = match &roles {
                Some(roles) => (roles.captured.iter())
                    .filter(|(_, role)| *role == Role::Begin)
                    .map(|(node, _)| *node)
                    .collect(),
                None => lists_in(tree.root_node(), start..end),
            };
            blocks.extend(
                candidates
                    .into_iter()
                    .filter_map(|node| block(&text, roles.as_ref(), node))
                    .filter(|block| block.lines.start < lines.end && lines.start < block.lines.end),
            );
        }
        // The outermost of those starting on one line.
        blocks.sort_by_key(|block| (block.lines.start, std::cmp::Reverse(block.lines.end)));
        blocks.dedup_by_key(|block| block.lines.start);
        Some(blocks)
    }

    fn query(&mut self, grammar: &Grammar, indents: &Indents) -> Option<Rc<IndentQuery>> {
        // Before the cache: a grammar not downloaded yet is not without indents for good.
        let language = grammar.language()?;
        self.queries
            .entry(grammar.mode)
            .or_insert_with(|| {
                IndentQuery::new(&language, indents.queries)
                    .inspect_err(|error| eprintln!("remacs: {} indents: {error}", grammar.mode))
                    .ok()
                    .map(Rc::new)
            })
            .clone()
    }

    /// `text` parsed again from `old`, its tree edited to follow it, in `old`'s ranges when
    /// it is code of another language.
    fn parse(
        &mut self,
        grammar: &'static Grammar,
        text: &Snapshot,
        old: Option<Tree>,
        terminator: Option<u8>,
    ) -> Option<Tree> {
        let ranges = old.as_ref().map(Tree::included_ranges).unwrap_or_default();
        let start = ranges.first().map_or(0, |range| range.start_byte);
        if let Some((mode, parsed_start, parsed, tree)) = &self.parsed
            && *mode == grammar.mode
            && *parsed_start == start
            && parsed.is_same(text)
        {
            return Some(tree.clone());
        }
        if old.is_none() && text.len() > MAX_FIRST_PARSE {
            return None;
        }
        let tree = parser_thread::parse_in(
            &mut self.parser,
            grammar,
            text,
            &ranges,
            old.as_ref(),
            terminator,
        )?;
        self.parsed = Some((grammar.mode, start, text.clone(), tree.clone()));
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
    let leaf = tree.root_node().descendant_for_byte_range(start, start + 1)?;
    if leaf.start_position().row < line {
        // Within a string or comment begun above: left as written.
        return Some(at_line(line, false));
    }
    let roles = query.roles_at(tree, text, start..start + 1);
    let closes = ancestors(leaf)
        .take_while(|node| node.start_byte() == start)
        .any(|node| roles.has(node, Role::Close));
    let outer = ancestors(leaf).take_while(|node| !node.is_error()).find(|node| {
        node.start_position().row < line
            && (roles.has(*node, Role::Begin) || roles.has(*node, Role::Align))
    });
    if outer.is_none() && ancestors(leaf).any(|node| node.is_error()) {
        return None;
    }
    Some(outer.map_or_else(Indentation::default, |outer| within(text, &roles, outer, !closes)))
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
    Indentation { anchor: Some(anchor), deeper, align: 0 }
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
    Indentation { align: String::from_utf8_lossy(&before).chars().count(), ..at_line(row, false) }
}

/// The block `node` is, over more than one line: the lines after its first are hidden,
/// but its last when it starts with the node's closing token, like `}` or `end`.
fn block(text: &Snapshot, roles: Option<&Roles>, node: Node) -> Option<Block> {
    let first = node.start_position().row;
    let end = node.end_position();
    // Ending with its line's newline, it ends on that line.
    let last = if end.column == 0 { end.row.checked_sub(1)? } else { end.row };
    if last <= first {
        return None;
    }
    let closer = (node.child_count().checked_sub(1)).and_then(|index| node.child(index));
    let closes = closer.is_some_and(|closer| {
        let starts_line = (text.line_range(last))
            .and_then(|line| first_code(text, line))
            .is_some_and(|start| start == closer.start_byte());
        let closing =
            !closer.is_named() || roles.is_some_and(|roles| roles.has(closer, Role::Close));
        starts_line && closing
    });
    Some(Block { lines: first..last + 1, hidden: first + 1..if closes { last } else { last + 1 } })
}

/// The lists between brackets under `root` that touch `range`.
fn lists_in(root: Node, range: Range<usize>) -> Vec<Node> {
    let mut lists = Vec::new();
    let mut nodes = vec![root];
    while let Some(node) = nodes.pop() {
        if node.end_byte() < range.start || node.start_byte() > range.end {
            continue;
        }
        if sexp::is_list(&node) {
            lists.push(node);
        }
        // Only the children touching the range: a file's root can have a hundred thousand.
        let mut cursor = node.walk();
        if cursor.goto_first_child_for_byte(range.start).is_some() {
            while cursor.node().start_byte() <= range.end {
                nodes.push(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
    lists
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
    (line.saturating_sub(MAX_BLANK_LINES)..line).rev().find_map(|above| {
        let range = text.line_range(above)?;
        let start = range.start.max(range.end.saturating_sub(MAX_SCAN));
        let tail = text.bytes_in(start..range.end);
        let end = tail.iter().rposition(|byte| !byte.is_ascii_whitespace())?;
        Some((above, start + end + 1))
    })
}
