use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use remacs_core::Notifier;
use remacs_text::Snapshot;
use streaming_iterator::StreamingIterator;
use tree_sitter::{InputEdit, Language, Parser, Query, QueryCursor, Tree};

use crate::highlighter::{Chunks, Highlighter};
use crate::queries::UserQueries;
use crate::{Grammar, Injections};

/// Where the parser thread leaves a buffer's newest tree.
pub(crate) type ParseSlot = Arc<Mutex<Option<Parse>>>;

pub(crate) struct Request {
    pub grammar: &'static Grammar,
    /// Every grammar, which injected languages are found among.
    pub grammars: &'static [Grammar],
    /// Every mode's injections, followed into injected code too; none when turned off.
    pub injections: &'static [Injections],
    pub queries: UserQueries,
    pub snapshot: Snapshot,
    /// The previous trees, already edited to match `snapshot`, so parsing is incremental.
    pub tree: Option<Tree>,
    pub layers: Vec<Layer>,
    /// Where the text changed since the layers were found, in `snapshot`.
    pub edited: Option<Range<usize>>,
    pub slot: ParseSlot,
}

/// The code of one injected language, parsed in the host's ranges of it: those of one
/// match, like a tagged template, or when combined all of them, or all with a terminator,
/// like `onclick` attributes, each ended by it.
#[derive(Clone)]
pub(crate) struct Layer {
    pub grammar: &'static Grammar,
    pub tree: Tree,
    /// For each of the tree's included ranges, what the injection query matched to find
    /// it, e.g. a script's tag with its type; an edit there finds the range again.
    owners: Vec<Range<usize>>,
    pub combined: bool,
    pub terminator: Option<u8>,
    /// Painted over its host's faces rather than in place of them, and no language of
    /// the code for editing: a regex in a string, a TODO in a comment.
    pub overlay: bool,
    pub highlighter: Option<Arc<Highlighter>>,
    /// The languages injected into this one's code.
    pub layers: Vec<Layer>,
}

impl Layer {
    /// Where its code is, around all its ranges.
    pub fn bounds(&self) -> Range<usize> {
        self.owners[0].start..self.owners[self.owners.len() - 1].end
    }

    fn kind(&self) -> Kind {
        kind(
            self.grammar,
            self.combined,
            self.terminator,
            &self.owners[0],
        )
    }

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
        for layer in &mut self.layers {
            layer.edit(edit);
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
            compiled.use_queries(&request.queries);
            let old_tree = request.tree.as_ref();
            let Some(tree) = parse_in(
                &mut parser,
                request.grammar,
                &request.snapshot,
                &[],
                old_tree,
                None,
            ) else {
                continue;
            };
            let layers = Injector {
                parser: &mut parser,
                compiled: &mut compiled,
                grammars: request.grammars,
                injections: request.injections,
                text: &request.snapshot,
                edited: request.edited,
            }
            .layers(
                request.grammar,
                &tree,
                old_tree.map(|old| (old, request.layers)),
            );
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
    combined: bool,
    terminator: Option<u8>,
    overlay: bool,
}

impl Region {
    fn kind(&self) -> Kind {
        kind(self.grammar, self.combined, self.terminator, &self.owner)
    }
}

/// Which layer code goes to: a language's combined code, with or without a terminator, or
/// one match's.
type Kind = (*const Grammar, Option<u8>, Option<Range<usize>>);

fn kind(grammar: &Grammar, combined: bool, terminator: Option<u8>, owner: &Range<usize>) -> Kind {
    (grammar, terminator, (!combined).then(|| owner.clone()))
}

/// Parses the languages injected into a tree, and those injected into them in turn.
struct Injector<'a> {
    parser: &'a mut Parser,
    compiled: &'a mut Compiled,
    grammars: &'static [Grammar],
    injections: &'static [Injections],
    text: &'a Snapshot,
    /// Where the text changed since the layers were found.
    edited: Option<Range<usize>>,
}

impl Injector<'_> {
    /// The languages `grammar`'s query injects into `tree`, each parsed in its ranges from
    /// its previous tree; `old` is the tree before, edited, with the layers found in it.
    /// Only where the text or the tree changed is the query run again, as running it over
    /// a large page takes longer than parsing the change.
    fn layers(
        &mut self,
        grammar: &'static Grammar,
        tree: &Tree,
        old: Option<(&Tree, Vec<Layer>)>,
    ) -> Vec<Layer> {
        let Some(query) = self
            .compiled
            .injections(self.injections, grammar, self.grammars)
        else {
            return Vec::new();
        };
        let (old_tree, old_layers) = match old {
            Some((old_tree, old_layers)) => (Some(old_tree), old_layers),
            None => (None, Vec::new()),
        };
        // Layers whose code no change touched, kept whole rather than taken apart into
        // regions and put together again; and those parsed again, from their old trees.
        let mut untouched: Vec<Layer> = Vec::new();
        let mut reparsed: HashMap<Kind, Layer> = HashMap::new();
        let mut regions =
            match old_tree {
                None => query.regions(tree, self.text, None),
                Some(old_tree) => {
                    let mut changed: Vec<Range<usize>> = old_tree
                        .changed_ranges(tree)
                        .map(|range| range.start_byte..range.end_byte)
                        .chain(self.edited.clone())
                        .collect();
                    let mut kept = Vec::new();
                    for layer in old_layers {
                        let touched = |owner: &Range<usize>| {
                            changed.iter().any(|range| touches(range, owner))
                        };
                        if !layer.owners.iter().any(touched) {
                            untouched.push(layer);
                            continue;
                        }
                        let ranges = layer.tree.included_ranges();
                        for (owner, content) in layer.owners.iter().zip(ranges) {
                            if changed.iter().any(|range| touches(range, owner)) {
                                changed.push(owner.clone());
                            } else {
                                kept.push(Region {
                                    grammar: layer.grammar,
                                    owner: owner.clone(),
                                    content,
                                    combined: layer.combined,
                                    terminator: layer.terminator,
                                    overlay: layer.overlay,
                                });
                            }
                        }
                        reparsed.insert(layer.kind(), layer);
                    }
                    for range in changed {
                        kept.extend(query.regions(tree, self.text, Some(range)));
                    }
                    // One that gains code, like a new attribute, is parsed again with it.
                    let gaining: HashSet<Kind> = kept.iter().map(Region::kind).collect();
                    for layer in untouched.extract_if(.., |layer| gaining.contains(&layer.kind())) {
                        let ranges = layer.tree.included_ranges();
                        kept.extend(layer.owners.iter().zip(ranges).map(|(owner, content)| {
                            Region {
                                grammar: layer.grammar,
                                owner: owner.clone(),
                                content,
                                combined: layer.combined,
                                terminator: layer.terminator,
                                overlay: layer.overlay,
                            }
                        }));
                        reparsed.insert(layer.kind(), layer);
                    }
                    kept
                }
            };
        regions.sort_by_key(|region| region.content.start_byte);
        regions.dedup_by_key(|region| region.content.start_byte);
        let mut groups: Vec<Vec<&Region>> = Vec::new();
        let mut group_of: HashMap<Kind, usize> = HashMap::new();
        for region in &regions {
            let group = *group_of.entry(region.kind()).or_insert_with(|| {
                groups.push(Vec::new());
                groups.len() - 1
            });
            groups[group].push(region);
        }
        let mut layers = untouched;
        for group in groups {
            let (owners, ranges): (Vec<_>, Vec<_>) = (group.iter())
                .map(|region| (region.owner.clone(), region.content))
                .unzip();
            let old = reparsed.remove(&group[0].kind());
            layers.extend(self.layer(group[0], owners, &ranges, old));
        }
        // In order of their code, so that the documents of embedded code keep theirs.
        layers.sort_by_key(|layer| layer.owners[0].start);
        layers
    }

    /// The code of `first`'s kind in `ranges`, parsed from `old`'s tree, with what it
    /// injects. Kept as it was when nothing changed in its code: reparsing a tree of many
    /// pieces takes time in proportion to them, even where nothing changed.
    fn layer(
        &mut self,
        first: &Region,
        owners: Vec<Range<usize>>,
        ranges: &[tree_sitter::Range],
        old: Option<Layer>,
    ) -> Option<Layer> {
        let untouched = |old: &Layer| {
            old.owners == owners
                && (self.edited.as_ref())
                    .is_none_or(|edited| !owners.iter().any(|owner| touches(edited, owner)))
        };
        if old.as_ref().is_some_and(untouched) {
            return old;
        }
        let (grammar, combined, terminator, overlay) = (
            first.grammar,
            first.combined,
            first.terminator,
            first.overlay,
        );
        let (old_tree, old_layers) = match old {
            Some(old) => (Some(old.tree), old.layers),
            None => (None, Vec::new()),
        };
        let tree = parse_in(
            self.parser,
            grammar,
            self.text,
            ranges,
            old_tree.as_ref(),
            terminator,
        )?;
        let layers = self.layers(
            grammar,
            &tree,
            old_tree.as_ref().map(|old_tree| (old_tree, old_layers)),
        );
        Some(Layer {
            grammar,
            tree,
            owners,
            combined,
            terminator,
            overlay,
            highlighter: self.compiled.highlighter(grammar),
            layers,
        })
    }
}

/// Whether `range` overlaps or borders `owner`, which a deletion's empty range does too.
fn touches(range: &Range<usize>, owner: &Range<usize>) -> bool {
    range.start <= owner.end && owner.start <= range.end
}

/// `text` parsed with `grammar` in `ranges`, all of it when there are none. With a
/// `terminator`, each range's first and last bytes, its delimiters, are read as spaces,
/// or the last as the terminator when the code between them does not end with it.
// shortcut: a string, template or comment a piece leaves open runs on into the next
// pieces, a line comment into the rest of its line; a tree per piece costs 4.6 ms a
// keystroke on a page of thousands, so split the layer into small trees if that matters.
fn parse_in(
    parser: &mut Parser,
    grammar: &Grammar,
    text: &Snapshot,
    ranges: &[tree_sitter::Range],
    old_tree: Option<&Tree>,
    terminator: Option<u8>,
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
    let Some(terminator) = terminator else {
        return parse(parser, text, old_tree);
    };
    let read_as: Vec<(usize, u8)> = ranges
        .iter()
        .filter(|range| range.end_byte - range.start_byte >= 2)
        .flat_map(|range| {
            let (open, close) = (range.start_byte, range.end_byte - 1);
            let last = (text.chunks(open + 1..close).flatten())
                .filter(|byte| !byte.is_ascii_whitespace())
                .last();
            let closing = match last {
                Some(&last) if last != terminator => terminator,
                _ => b' ',
            };
            [(open, b' '), (close, closing)]
        })
        .collect();
    let read_as = &read_as;
    parser.parse_with_options(
        &mut |offset, _| {
            let next = read_as.partition_point(|&(at, _)| at < offset);
            match read_as.get(next) {
                Some((at, byte)) if *at == offset => std::slice::from_ref(byte),
                Some(&(at, _)) => text.chunks(offset..at).next().unwrap_or_default(),
                None => text.chunks(offset..text.len()).next().unwrap_or_default(),
            }
        },
        old_tree,
        None,
    )
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
    /// The user's queries the highlighters were compiled with.
    queries: UserQueries,
}

impl Compiled {
    /// Compiles highlighters with `queries` from now on, again when they changed.
    fn use_queries(&mut self, queries: &UserQueries) {
        if !Arc::ptr_eq(&self.queries, queries) {
            self.queries = Arc::clone(queries);
            self.highlighters.clear();
        }
    }

    fn highlighter(&mut self, grammar: &'static Grammar) -> Option<Arc<Highlighter>> {
        let own = self.queries.get(grammar.mode).map(String::as_str);
        self.highlighters
            .entry(grammar.mode)
            .or_insert_with(|| {
                let own = own.map(|own| [own]);
                let sources = own.as_ref().map_or(grammar.highlights, |own| own);
                Highlighter::new(&Language::new(grammar.language), sources)
                    .inspect_err(|error| eprintln!("remacs: {} highlights: {error}", grammar.mode))
                    .ok()
                    .map(Arc::new)
            })
            .clone()
    }

    fn injections(
        &mut self,
        injections: &'static [Injections],
        host: &'static Grammar,
        grammars: &'static [Grammar],
    ) -> Option<Arc<InjectionQuery>> {
        // Looked up first, so that injections turned off do not cache their absence.
        let injections = injections
            .iter()
            .find(|injections| injections.host == host.mode)?;
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

/// The grammar of the mode named `name`, ignoring case.
fn grammar_named(grammars: &'static [Grammar], name: &[u8]) -> Option<&'static Grammar> {
    grammars
        .iter()
        .find(|grammar| grammar.mode.as_bytes().eq_ignore_ascii_case(name))
}

/// What one pattern of an injection query injects.
struct Injection {
    /// Its `#set! injection.language`; `None` reads it from the `@injection.language` capture.
    language: Option<Option<&'static Grammar>>,
    combined: bool,
    terminator: Option<u8>,
    overlay: bool,
}

/// A compiled injection query, with what each of its patterns injects.
struct InjectionQuery {
    query: Query,
    content: u32,
    language: Option<u32>,
    grammars: &'static [Grammar],
    injections: Vec<Injection>,
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
        let injections = (0..query.pattern_count())
            .map(|pattern| {
                let properties = query.property_settings(pattern);
                let property = |key: &str| {
                    properties
                        .iter()
                        .find(|property| &*property.key == key)
                        .map(|property| property.value.as_deref().unwrap_or_default())
                };
                Injection {
                    language: property("injection.language")
                        .map(|name| grammar_named(grammars, name.as_bytes())),
                    combined: property("injection.combined").is_some(),
                    terminator: property("injection.terminator")
                        .and_then(|terminator| terminator.bytes().next()),
                    overlay: property("injection.overlay").is_some(),
                }
            })
            .collect();
        Ok(Self {
            language: query.capture_index_for_name("injection.language"),
            query,
            content,
            grammars,
            injections,
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
            let injection = &self.injections[matched.pattern_index];
            let grammar = injection.language.unwrap_or_else(|| {
                let name = matched
                    .captures()
                    .iter()
                    .find(|capture| Some(capture.index) == self.language)?
                    .node
                    .byte_range();
                grammar_named(self.grammars, &text.bytes_in(name))
            });
            let Some(grammar) = grammar else {
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
                        combined: injection.combined,
                        terminator: injection.terminator,
                        overlay: injection.overlay,
                    }),
            );
        }
        regions
    }
}
