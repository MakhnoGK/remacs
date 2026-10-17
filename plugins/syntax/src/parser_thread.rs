use std::collections::HashMap;
use std::ops::Range;
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use remacs_core::Notifier;
use remacs_text::Snapshot;
use streaming_iterator::StreamingIterator;
use tree_sitter::{InputEdit, Language, Parser, Query, QueryCursor, Tree};

use crate::highlighter::{Chunks, Highlighter};
use crate::{Grammar, Injections};

/// Where the parser thread leaves a buffer's newest tree.
pub(crate) type ParseSlot = Arc<Mutex<Option<Parse>>>;

pub(crate) struct Request {
    pub grammar: &'static Grammar,
    /// Every grammar, which injected languages are found among.
    pub grammars: &'static [Grammar],
    pub injections: Option<&'static Injections>,
    pub snapshot: Snapshot,
    /// The previous trees, already edited to match `snapshot`, so parsing is incremental.
    pub tree: Option<Tree>,
    pub layers: Vec<Layer>,
    /// Where the text changed since the layers were found, in `snapshot`.
    pub edited: Option<Range<usize>>,
    pub slot: ParseSlot,
}

/// The code of one injected language, parsed in the host's ranges of it.
#[derive(Clone)]
pub(crate) struct Layer {
    pub grammar: &'static Grammar,
    pub tree: Tree,
    /// For each of the tree's included ranges, what the injection query matched to find
    /// it, e.g. a script's tag with its type; an edit there finds the range again.
    owners: Vec<Range<usize>>,
    pub highlighter: Option<Arc<Highlighter>>,
}

impl Layer {
    pub fn edit(&mut self, edit: &InputEdit) {
        self.tree.edit(edit);
        let shift = |offset: usize| match offset {
            _ if offset <= edit.start_byte => offset,
            _ if offset >= edit.old_end_byte => offset - edit.old_end_byte + edit.new_end_byte,
            _ => edit.new_end_byte,
        };
        for owner in &mut self.owners {
            *owner = shift(owner.start)..shift(owner.end);
        }
    }
}

pub(crate) struct Parse {
    pub snapshot: Snapshot,
    pub tree: Tree,
    pub highlighter: Option<Arc<Highlighter>>,
    pub layers: Vec<Layer>,
}

/// One thread parses for every buffer; it starts with the first request.
#[derive(Default)]
pub(crate) struct ParserThread(Option<Sender<Request>>);

impl ParserThread {
    pub fn send(&mut self, request: Request, notifier: Notifier) {
        let requests = self.0.get_or_insert_with(|| {
            let (requests, received) = mpsc::channel();
            thread::Builder::new()
                .name("syntax".into())
                .spawn(move || run(received, notifier))
                .expect("spawning the syntax thread");
            requests
        });
        // The thread only stops when this sender is dropped.
        let _ = requests.send(request);
    }
}

fn run(requests: Receiver<Request>, notifier: Notifier) {
    let mut parser = Parser::new();
    let mut compiled = Compiled::default();
    while let Ok(first) = requests.recv() {
        for request in newest_per_buffer(first, &requests) {
            // The buffer was killed or stopped being parsed.
            if Arc::strong_count(&request.slot) == 1 {
                continue;
            }
            let old_tree = request.tree.as_ref();
            let Some(tree) = parse_in(
                &mut parser,
                request.grammar,
                &request.snapshot,
                &[],
                old_tree,
            ) else {
                continue;
            };
            let layers = request
                .injections
                .and_then(|injections| {
                    compiled.injections(injections, request.grammar, request.grammars)
                })
                .map(|query| parse_layers(&mut parser, &mut compiled, &query, &tree, &request))
                .unwrap_or_default();
            *request.slot.lock().expect("parse slots are never poisoned") = Some(Parse {
                snapshot: request.snapshot,
                tree,
                highlighter: compiled.highlighter(request.grammar),
                layers,
            });
            notifier();
        }
    }
}

/// Requests queued while the previous parse ran; older ones for the same buffer are stale.
fn newest_per_buffer(first: Request, requests: &Receiver<Request>) -> Vec<Request> {
    let mut newest: Vec<Request> = Vec::new();
    for request in std::iter::once(first).chain(requests.try_iter()) {
        match newest
            .iter_mut()
            .find(|queued| Arc::ptr_eq(&queued.slot, &request.slot))
        {
            Some(queued) => *queued = request,
            None => newest.push(request),
        }
    }
    newest
}

/// A range an injected grammar parses, and what the query matched to find it.
struct Region {
    grammar: &'static Grammar,
    owner: Range<usize>,
    content: tree_sitter::Range,
}

/// The injected languages of `tree`, each parsed in its ranges from its previous tree.
/// Only where the text or the host's tree changed is the injection query run again, as
/// running it over a large page takes longer than parsing the change.
fn parse_layers(
    parser: &mut Parser,
    compiled: &mut Compiled,
    query: &InjectionQuery,
    tree: &Tree,
    request: &Request,
) -> Vec<Layer> {
    let text = &request.snapshot;
    let mut regions = match &request.tree {
        None => query.regions(tree, text, None),
        Some(old_tree) => {
            let mut changed: Vec<Range<usize>> = old_tree
                .changed_ranges(tree)
                .map(|range| range.start_byte..range.end_byte)
                .chain(request.edited.clone())
                .collect();
            let mut kept = Vec::new();
            for layer in &request.layers {
                let ranges = layer.tree.included_ranges();
                for (owner, content) in layer.owners.iter().zip(ranges) {
                    if changed.iter().any(|range| touches(range, owner)) {
                        changed.push(owner.clone());
                    } else {
                        kept.push(Region {
                            grammar: layer.grammar,
                            owner: owner.clone(),
                            content,
                        });
                    }
                }
            }
            for range in changed {
                kept.extend(query.regions(tree, text, Some(range)));
            }
            kept
        }
    };
    regions.sort_by_key(|region| region.content.start_byte);
    regions.dedup_by_key(|region| region.content.start_byte);
    let mut grammars: Vec<&'static Grammar> = Vec::new();
    for region in &regions {
        if !grammars
            .iter()
            .any(|grammar| ptr::eq(*grammar, region.grammar))
        {
            grammars.push(region.grammar);
        }
    }
    grammars
        .into_iter()
        .filter_map(|grammar| {
            let (owners, ranges): (Vec<_>, Vec<_>) = regions
                .iter()
                .filter(|region| ptr::eq(region.grammar, grammar))
                .map(|region| (region.owner.clone(), region.content))
                .unzip();
            let old_tree = request
                .layers
                .iter()
                .find(|layer| ptr::eq(layer.grammar, grammar))
                .map(|layer| &layer.tree);
            let tree = parse_in(parser, grammar, text, &ranges, old_tree)?;
            Some(Layer {
                grammar,
                tree,
                owners,
                highlighter: compiled.highlighter(grammar),
            })
        })
        .collect()
}

/// Whether `range` overlaps or borders `owner`, which a deletion's empty range does too.
fn touches(range: &Range<usize>, owner: &Range<usize>) -> bool {
    range.start <= owner.end && owner.start <= range.end
}

/// `text` parsed with `grammar` in `ranges`, all of it when there are none.
fn parse_in(
    parser: &mut Parser,
    grammar: &Grammar,
    text: &Snapshot,
    ranges: &[tree_sitter::Range],
    old_tree: Option<&Tree>,
) -> Option<Tree> {
    if parser
        .set_language(&Language::new(grammar.language))
        .is_err()
    {
        eprintln!("remacs: {} grammar is incompatible", grammar.mode);
        return None;
    }
    parser
        .set_included_ranges(ranges)
        .inspect_err(|error| eprintln!("remacs: {} ranges: {error}", grammar.mode))
        .ok()?;
    parse(parser, text, old_tree)
}

pub(crate) fn parse(parser: &mut Parser, text: &Snapshot, old_tree: Option<&Tree>) -> Option<Tree> {
    parser.parse_with_options(
        &mut |offset, _| text.chunks(offset..text.len()).next().unwrap_or_default(),
        old_tree,
        None,
    )
}

/// Queries compiled on first use, once per mode; `None` for one that does not compile.
#[derive(Default)]
struct Compiled {
    highlighters: HashMap<&'static str, Option<Arc<Highlighter>>>,
    injections: HashMap<&'static str, Option<Arc<InjectionQuery>>>,
}

impl Compiled {
    fn highlighter(&mut self, grammar: &'static Grammar) -> Option<Arc<Highlighter>> {
        self.highlighters
            .entry(grammar.mode)
            .or_insert_with(|| {
                Highlighter::new(&Language::new(grammar.language), grammar.highlights)
                    .inspect_err(|error| eprintln!("remacs: {} highlights: {error}", grammar.mode))
                    .ok()
                    .map(Arc::new)
            })
            .clone()
    }

    fn injections(
        &mut self,
        injections: &'static Injections,
        host: &'static Grammar,
        grammars: &'static [Grammar],
    ) -> Option<Arc<InjectionQuery>> {
        self.injections
            .entry(host.mode)
            .or_insert_with(|| {
                InjectionQuery::new(&Language::new(host.language), injections.query, grammars)
                    .inspect_err(|error| eprintln!("remacs: {} injections: {error}", host.mode))
                    .ok()
                    .map(Arc::new)
            })
            .clone()
    }
}

/// A compiled injection query, with the grammar each of its patterns injects.
struct InjectionQuery {
    query: Query,
    content: u32,
    languages: Vec<Option<&'static Grammar>>,
}

impl InjectionQuery {
    fn new(
        language: &Language,
        source: &str,
        grammars: &'static [Grammar],
    ) -> Result<Self, String> {
        let query = Query::new(language, source).map_err(|error| error.to_string())?;
        let content = query
            .capture_index_for_name("injection.content")
            .ok_or("no @injection.content capture")?;
        let languages = (0..query.pattern_count())
            .map(|pattern| {
                let name = query
                    .property_settings(pattern)
                    .iter()
                    .find(|property| &*property.key == "injection.language")?
                    .value
                    .as_deref()?;
                grammars
                    .iter()
                    .find(|grammar| grammar.mode.eq_ignore_ascii_case(name))
            })
            .collect();
        Ok(Self {
            query,
            content,
            languages,
        })
    }

    /// The regions the query finds in `tree`, within `range` or everywhere.
    fn regions(&self, tree: &Tree, text: &Snapshot, range: Option<Range<usize>>) -> Vec<Region> {
        let mut regions = Vec::new();
        let mut cursor = QueryCursor::new();
        if let Some(range) = range {
            cursor.set_byte_range(range);
        }
        let mut matches = cursor.matches(&self.query, tree.root_node(), Chunks(text));
        while let Some(matched) = matches.next() {
            let Some(grammar) = self.languages[matched.pattern_index] else {
                continue;
            };
            let captures = matched.captures();
            let start = captures
                .iter()
                .map(|capture| capture.node.start_byte())
                .min();
            let end = captures.iter().map(|capture| capture.node.end_byte()).max();
            let (Some(start), Some(end)) = (start, end) else {
                continue;
            };
            regions.extend(
                captures
                    .iter()
                    .filter(|capture| capture.index == self.content)
                    .map(|capture| Region {
                        grammar,
                        owner: start..end,
                        content: capture.node.range(),
                    }),
            );
        }
        regions
    }
}
