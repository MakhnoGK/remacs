//! A function's parameters and local variables highlighted where they are defined and
//! wherever they are used in it, as each mode's locals query captures them (ADR 0086):
//! `@local.scope` a function, closure or block, `@local.definition` a name it defines,
//! `@local.reference` a name that may use one, `@local.skip` a name that never does, like
//! a field's after a dot. A use is a local's when a scope around it defines its name
//! before it.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

use remacs_core::{Buffer, Face, Span};
use remacs_text::Snapshot;
use streaming_iterator::StreamingIterator;
use tree_sitter::{CaptureQuantifier, Language, Node, Query, QueryCursor, Tree};

use crate::buffer_syntax;
use crate::highlighter::Chunks;
use crate::{Grammar, Locals};

/// How far from the lines drawn their scopes' definitions are read, at least.
const NEAR_BYTES: usize = 16 << 10;
/// Scopes' names kept: the lines drawn on each side of a fold, in each window.
const SCOPES_KEPT: usize = 8;

pub(crate) struct LocalsQuery {
    query: Query,
    /// Its patterns of scopes and definitions only, to read whole scopes by.
    definitions_query: Query,
    scope: u32,
    definition: u32,
    reference: u32,
    skip: Option<u32>,
    /// The scopes' names last read, kept while the tree and its scopes around the lines
    /// drawn stay: frames scrolling a function read them once, the newest last.
    defined: RefCell<Vec<Defined>>,
}

/// The names the scopes in `scopes` of a tree define, wherever in each: a comprehension
/// uses its names before its `for`.
struct Defined {
    text: Snapshot,
    root: usize,
    scopes: Range<usize>,
    /// Each scope's bytes and names, of those defining some.
    names: Vec<(Range<usize>, HashSet<Vec<u8>>)>,
}

/// Whether `node` is within `scope`, and not the scope itself: a closure's own name is
/// no name it defines.
fn holds(scope: &Range<usize>, node: &Node) -> bool {
    let bytes = node.byte_range();
    scope.start <= bytes.start && bytes.end <= scope.end && bytes != *scope
}

impl LocalsQuery {
    pub fn new(language: &Language, sources: &[&str]) -> Result<Self, String> {
        let source = sources.join("\n");
        let query = Query::new(language, &source).map_err(|error| error.to_string())?;
        let index = |name| {
            query
                .capture_index_for_name(name)
                .ok_or_else(|| format!("no @{name}"))
        };
        let (scope, definition) = (index("local.scope")?, index("local.definition")?);
        let mut definitions_query =
            Query::new(language, &source).map_err(|error| error.to_string())?;
        for pattern in 0..definitions_query.pattern_count() {
            let captures = definitions_query.capture_quantifiers(pattern);
            let defines = [scope, definition]
                .iter()
                .any(|&capture| captures[capture as usize] != CaptureQuantifier::Zero);
            if !defines {
                definitions_query.disable_pattern(pattern);
            }
        }
        Ok(Self {
            scope,
            definition,
            reference: index("local.reference")?,
            skip: query.capture_index_for_name("local.skip"),
            query,
            definitions_query,
            defined: RefCell::new(Vec::new()),
        })
    }

    /// The locals' definitions and uses in `range` of `tree`, reading the whole scopes
    /// around it for their definitions.
    fn spans(&self, tree: &Tree, text: &Snapshot, range: Range<usize>) -> Vec<Span> {
        // The scopes around the range are captured too, ancestors touching it.
        let mut scopes: Option<Range<usize>> = None;
        let mut skipped = HashSet::new();
        let mut definitions = Vec::new();
        let mut references = Vec::new();
        Self::capture(&self.query, tree, text, range.clone(), |node, capture| {
            if capture == self.scope {
                let bytes = node.byte_range();
                scopes = Some(match scopes.take() {
                    Some(around) => around.start.min(bytes.start)..around.end.max(bytes.end),
                    None => bytes,
                });
            } else if capture == self.definition {
                definitions.push(node);
            } else if Some(capture) == self.skip {
                skipped.insert(node.id());
            } else if capture == self.reference {
                references.push(node);
            }
        });
        let Some(scopes) = scopes else {
            return Vec::new();
        };
        // shortcut: a scope as large as a bundled file, one function, is read only near the
        // lines drawn, in steps that stay put while they scroll; its names defined farther
        // off are not colored there. Read them all if that is missed.
        let near = (range.start / NEAR_BYTES).saturating_sub(1) * NEAR_BYTES
            ..(range.end / NEAR_BYTES + 2) * NEAR_BYTES;
        let scopes = scopes.start.max(near.start)..scopes.end.min(near.end);
        references.retain(|reference| !skipped.contains(&reference.id()));
        let mut defined = self.defined.borrow_mut();
        let root = tree.root_node().id();
        let known = (defined.iter()).position(|defined| {
            defined.text.is_same(text) && defined.root == root && defined.scopes == scopes
        });
        let index = known.unwrap_or_else(|| {
            if defined.len() == SCOPES_KEPT {
                defined.remove(0);
            }
            defined.push(self.defined_in(tree, text, scopes));
            defined.len() - 1
        });
        let names = &defined[index].names;
        // By their bytes: a node's parent is found from the root down, each time.
        let in_scope = |node: &Node| names.iter().any(|(scope, _)| holds(scope, node));
        let is_local = |reference: &Node| {
            let name = text.bytes_in(reference.byte_range());
            (names.iter()).any(|(scope, names)| holds(scope, reference) && names.contains(&name))
        };
        let mut spans: Vec<Span> = (definitions.iter())
            .filter(|definition| in_scope(definition))
            .chain(references.iter().filter(|reference| is_local(reference)))
            .map(|node| Span {
                range: node.start_byte().max(range.start)..node.end_byte().min(range.end),
                face: Face::Variable,
            })
            .filter(|span| !span.range.is_empty())
            .collect();
        spans.sort_by_key(|span| span.range.start);
        spans.dedup_by_key(|span| span.range.start);
        spans
    }

    /// The names the scopes in `scopes` define, read with the definitions query; each
    /// definition is its innermost scope's.
    fn defined_in(&self, tree: &Tree, text: &Snapshot, scopes: Range<usize>) -> Defined {
        let mut names: Vec<(Range<usize>, HashSet<Vec<u8>>)> = Vec::new();
        let mut definitions = Vec::new();
        Self::capture(
            &self.definitions_query,
            tree,
            text,
            scopes.clone(),
            |node, capture| {
                if capture == self.scope {
                    names.push((node.byte_range(), HashSet::new()));
                } else if capture == self.definition {
                    definitions.push(node);
                }
            },
        );
        names.sort_by_key(|(scope, _)| (scope.start, std::cmp::Reverse(scope.end)));
        names.dedup_by(|later, earlier| later.0 == earlier.0);
        for definition in definitions {
            let innermost = (names.iter_mut())
                .filter(|(scope, _)| holds(scope, &definition))
                .min_by_key(|(scope, _)| scope.len());
            if let Some((_, names)) = innermost {
                names.insert(text.bytes_in(definition.byte_range()));
            }
        }
        names.retain(|(_, names)| !names.is_empty());
        Defined {
            text: text.clone(),
            root: tree.root_node().id(),
            scopes,
            names,
        }
    }

    fn capture<'tree>(
        query: &Query,
        tree: &'tree Tree,
        text: &Snapshot,
        range: Range<usize>,
        mut each: impl FnMut(Node<'tree>, u32),
    ) {
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range);
        // Matches in any order: captures in order of their nodes would wait on each scope,
        // which spans the whole range, and cost several times as much.
        let mut matches = cursor.matches(query, tree.root_node(), Chunks(text));
        while let Some(matched) = matches.next() {
            for capture in matched.captures() {
                each(capture.node, capture.index);
            }
        }
    }
}

/// Each mode's locals query, compiled on first use; `None` for one without one, or
/// whose query does not compile.
#[derive(Default)]
pub(crate) struct LocalsQueries(HashMap<&'static str, Option<LocalsQuery>>);

impl LocalsQueries {
    /// `spans` of `range` of the buffer with its locals painted over them where nothing
    /// else is: a local function called keeps the call's face.
    pub fn paint(
        &mut self,
        locals: &'static [Locals],
        buffer: &Buffer,
        range: Range<usize>,
        spans: Vec<Span>,
    ) -> Vec<Span> {
        let Some((grammar, tree, text)) = buffer_syntax::own_tree(buffer) else {
            return spans;
        };
        // A page's scripts are their own language's code, with their own locals.
        let trees =
            std::iter::once((grammar, tree)).chain(buffer_syntax::layers_in(buffer, &range));
        let mut local = Vec::new();
        for (grammar, tree) in trees {
            if let Some(query) = self.query(locals, grammar) {
                local.extend(query.spans(tree, text, range.clone()));
            }
        }
        if local.is_empty() {
            return spans;
        }
        local.sort_by_key(|span| span.range.start);
        let painted: Vec<Range<usize>> = spans.iter().map(|span| span.range.clone()).collect();
        let mut spans = spans;
        spans.extend(buffer_syntax::outside(local, &painted));
        spans.sort_by_key(|span| span.range.start);
        spans
    }

    fn query(&mut self, locals: &'static [Locals], grammar: &Grammar) -> Option<&LocalsQuery> {
        // Before the cache: a grammar not downloaded yet is not without locals for good.
        let language = grammar.language()?;
        self.0
            .entry(grammar.mode)
            .or_insert_with(|| {
                let locals = locals.iter().find(|locals| locals.mode == grammar.mode)?;
                LocalsQuery::new(&language, locals.queries)
                    .inspect_err(|error| eprintln!("remacs: {} locals: {error}", grammar.mode))
                    .ok()
            })
            .as_ref()
    }
}
