use std::ops::Range;
use std::ptr;
use std::sync::Arc;

use remacs_core::{Buffer, Editor, Embedded, Span, SyntaxContext};
use remacs_text::{Edit, Snapshot};
use tree_sitter::{InputEdit, Point, Tree};

use crate::fallback::{self, Fallback};
use crate::highlighter::Highlighter;
use crate::parser_thread::{Layer, ParseSlot, ParserThread, Request};
use crate::queries::{Queries, UserQueries};
use crate::{Grammar, Injections};

/// Larger texts stay unparsed: a full parse would take seconds and its tree hundreds of MiB.
const MAX_PARSED_BYTES: usize = 16 << 20;

/// A buffer's syntax trees: its own and those of the languages injected into it. Edits
/// are applied to them as they happen, so their positions match the text at once; the
/// parser thread then replaces them with a fresh parse.
struct BufferSyntax {
    grammar: &'static Grammar,
    grammars: &'static [Grammar],
    injections: &'static [Injections],
    queries: UserQueries,
    /// The text the trees match.
    snapshot: Snapshot,
    tree: Option<Tree>,
    highlighter: Option<Arc<Highlighter>>,
    layers: Vec<Layer>,
    /// The text the layers were found in.
    layers_snapshot: Option<Snapshot>,
    slot: ParseSlot,
}

impl BufferSyntax {
    fn new(
        grammar: &'static Grammar,
        grammars: &'static [Grammar],
        injections: &'static [Injections],
        queries: UserQueries,
        snapshot: Snapshot,
    ) -> Self {
        Self {
            grammar,
            grammars,
            injections,
            queries,
            snapshot,
            tree: None,
            highlighter: None,
            layers: Vec::new(),
            layers_snapshot: None,
            slot: ParseSlot::default(),
        }
    }

    fn request(&self) -> Request {
        let edited = self.layers_snapshot.as_ref().and_then(|layers_snapshot| {
            let edit = self.snapshot.edit_from(layers_snapshot)?;
            Some(edit.start..edit.new_end)
        });
        Request {
            grammar: self.grammar,
            grammars: self.grammars,
            injections: self.injections,
            queries: Arc::clone(&self.queries),
            snapshot: self.snapshot.clone(),
            tree: self.tree.clone(),
            layers: self.layers.clone(),
            edited,
            slot: Arc::clone(&self.slot),
        }
    }

    /// Takes the parser thread's result and catches up with `text`; returns a request
    /// when the text changed and needs parsing again.
    fn update(&mut self, text: Snapshot) -> Option<Request> {
        self.take_parse();
        let edit = text.edit_from(&self.snapshot)?;
        self.edit(&input_edit(edit, &self.snapshot, &text));
        self.snapshot = text;
        Some(self.request())
    }

    fn edit(&mut self, edit: &InputEdit) {
        if let Some(tree) = &mut self.tree {
            tree.edit(edit);
        }
        for layer in &mut self.layers {
            layer.edit(edit);
        }
    }

    fn take_parse(&mut self) {
        let Some(parse) = self
            .slot
            .lock()
            .expect("parse slots are never poisoned")
            .take()
        else {
            return;
        };
        self.tree = Some(parse.tree);
        self.highlighter = parse.highlighter;
        self.layers = parse.layers;
        if let Some(edit) = self.snapshot.edit_from(&parse.snapshot) {
            self.edit(&input_edit(edit, &parse.snapshot, &self.snapshot));
        }
        self.layers_snapshot = Some(parse.snapshot);
    }
}

/// Keeps the window buffer's trees current; runs after every command.
pub(crate) fn sync(
    editor: &mut Editor,
    grammars: &'static [Grammar],
    injections: &'static [Injections],
) {
    let queries = Arc::clone(&editor.plugin_state::<Queries>().highlights);
    let mode = editor.mode(editor.window_buffer().mode()).copied();
    let buffer = editor.window_buffer_mut();
    let grammar = grammars
        .iter()
        .find(|grammar| grammar.mode == buffer.mode())
        .filter(|_| buffer.text().len() <= MAX_PARSED_BYTES);
    let Some(grammar) = grammar else {
        buffer.remove_local::<BufferSyntax>();
        let stale = buffer
            .local::<Fallback>()
            .is_none_or(|fallback| fallback.mode != buffer.mode());
        if stale {
            match mode.as_ref().and_then(Fallback::of) {
                Some(fallback) => {
                    buffer.set_local(fallback);
                }
                None => buffer.remove_local::<Fallback>(),
            }
        }
        return;
    };
    buffer.remove_local::<Fallback>();
    let text = buffer.text().snapshot();
    let request = match buffer.local_mut::<BufferSyntax>() {
        Some(syntax)
            if ptr::eq(syntax.grammar, grammar)
                && ptr::eq(syntax.injections, injections)
                && Arc::ptr_eq(&syntax.queries, &queries) =>
        {
            syntax.update(text)
        }
        _ => {
            let syntax = BufferSyntax::new(grammar, grammars, injections, queries, text);
            Some(buffer.set_local(syntax).request())
        }
    };
    if let Some(request) = request {
        let notifier = editor.notifier();
        editor
            .plugin_state::<ParserThread>()
            .send(request, notifier);
    }
}

pub(crate) fn highlight(buffer: &Buffer, range: Range<usize>, level: u8) -> Vec<Span> {
    let Some(syntax) = buffer.local::<BufferSyntax>() else {
        return fallback::highlight(buffer, range, level);
    };
    match (&syntax.tree, &syntax.highlighter) {
        (Some(tree), Some(highlighter)) => {
            let spans = highlighter.spans(tree, &syntax.snapshot, range.clone(), level);
            paint_layers(&syntax.layers, &syntax.snapshot, &range, level, spans)
        }
        _ => Vec::new(),
    }
}

/// `spans` of the code around `layers` with each layer's own painted over them in its
/// ranges, and the layers injected into it over those.
fn paint_layers(
    layers: &[Layer],
    text: &Snapshot,
    range: &Range<usize>,
    level: u8,
    mut spans: Vec<Span>,
) -> Vec<Span> {
    let mut held = Vec::new();
    let mut injected = Vec::new();
    for layer in layers
        .iter()
        .filter(|layer| overlaps(&layer.bounds(), range))
    {
        let Some(highlighter) = &layer.highlighter else {
            continue;
        };
        let own = highlighter.spans(&layer.tree, text, range.clone(), level);
        let painted = paint_layers(&layer.layers, text, range, level, own);
        if layer.overlay {
            // Only where it has faces: the rest of the comment or string stays as it was.
            held.extend(painted.iter().map(|span| span.range.clone()));
        } else {
            let ranges = layer.tree.included_ranges();
            held.extend(
                ranges
                    .iter()
                    .map(|held| held.start_byte..held.end_byte)
                    .filter(|held| overlaps(held, range)),
            );
        }
        injected.extend(painted);
    }
    if held.is_empty() {
        return spans;
    }
    held.sort_by_key(|held| held.start);
    spans = outside(spans, &held);
    spans.extend(injected);
    spans.sort_by_key(|span| span.range.start);
    spans
}

/// The parts of `spans` outside the ranges `held`, both in order.
pub(crate) fn outside(spans: Vec<Span>, held: &[Range<usize>]) -> Vec<Span> {
    let mut kept = Vec::with_capacity(spans.len());
    let mut first = 0;
    for span in spans {
        while held
            .get(first)
            .is_some_and(|held| held.end <= span.range.start)
        {
            first += 1;
        }
        let mut start = span.range.start;
        for held in held[first..]
            .iter()
            .take_while(|held| held.start < span.range.end)
        {
            if start < held.start {
                kept.push(Span {
                    range: start..held.start,
                    face: span.face,
                });
            }
            start = start.max(held.end);
        }
        if start < span.range.end {
            kept.push(Span {
                range: start..span.range.end,
                face: span.face,
            });
        }
    }
    kept
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

/// The innermost of `layers` and the layers injected into them whose ranges hold `range`;
/// overlays are no language of the code.
fn layer_at<'a>(layers: &'a [Layer], range: &Range<usize>) -> Option<&'a Layer> {
    let layer = layers.iter().find(|layer| {
        let bounds = layer.bounds();
        !layer.overlay
            && bounds.start <= range.start
            && range.end <= bounds.end
            && layer
                .tree
                .included_ranges()
                .iter()
                .any(|held| held.start_byte <= range.start && range.end <= held.end_byte)
    })?;
    layer_at(&layer.layers, range).or(Some(layer))
}

impl BufferSyntax {
    /// The tree that holds `range`: an injected layer's, else the buffer's own.
    fn tree_at(&self, range: Range<usize>) -> Option<&Tree> {
        layer_at(&self.layers, &range)
            .map(|layer| &layer.tree)
            .or(self.tree.as_ref())
    }
}

/// The buffer's grammar, its own tree and the text the tree is edited to follow, as
/// highlighting reads them.
pub(crate) fn own_tree(buffer: &Buffer) -> Option<(&'static Grammar, &Tree, &Snapshot)> {
    let syntax = buffer.local::<BufferSyntax>()?;
    Some((syntax.grammar, syntax.tree.as_ref()?, &syntax.snapshot))
}

/// The buffer's syntax, while its trees match the text.
fn current(buffer: &Buffer) -> Option<&BufferSyntax> {
    let syntax = buffer.local::<BufferSyntax>()?;
    syntax
        .snapshot
        .is_same(&buffer.text().snapshot())
        .then_some(syntax)
}

/// The tree that holds `range`, an injected language's or the buffer's, while it matches
/// the text.
pub(crate) fn tree_at(buffer: &Buffer, range: Range<usize>) -> Option<&Tree> {
    current(buffer)?.tree_at(range)
}

/// The code of the languages injected into the buffer that is a document of its own;
/// none for a buffer whose language holds no such code, and `None` for one that does
/// until it is parsed and while its trees lag behind the text.
pub(crate) fn embedded(buffer: &Buffer) -> Option<Vec<Embedded>> {
    let Some(syntax) = buffer.local::<BufferSyntax>() else {
        return Some(Vec::new());
    };
    let Some(host) = (syntax.injections.iter())
        .find(|injections| injections.host == syntax.grammar.mode && injections.documents)
    else {
        return Some(Vec::new());
    };
    let servers = host.servers;
    let syntax = current(buffer).filter(|syntax| syntax.tree.is_some())?;
    // One document per language: its combined code.
    let layers =
        (syntax.layers.iter()).filter(|layer| layer.combined && layer.terminator.is_none());
    let embedded = layers.map(|layer| Embedded {
        mode: layer.grammar.mode,
        ranges: layer
            .tree
            .included_ranges()
            .iter()
            .map(|range| range.start_byte..range.end_byte)
            .collect(),
        servers,
    });
    Some(embedded.collect())
}

/// The string, comment or embedded markup the character before `offset` belongs to, read
/// from the tree of its language; `None` until the trees match the text.
pub(crate) fn context_at(buffer: &Buffer, offset: usize) -> Option<SyntaxContext> {
    let syntax = current(buffer)?;
    if offset == 0 {
        return syntax.tree.as_ref().map(|_| SyntaxContext::Code);
    }
    let tree = syntax.tree_at(offset - 1..offset)?;
    let mut node = tree
        .root_node()
        .descendant_for_byte_range(offset - 1, offset);
    while let Some(current) = node {
        let kind = current.kind();
        let (start, end) = (current.start_byte(), current.end_byte());
        // Code nested in strings and markup: `${…}`, `#{…}`, JSX `{…}` and attributes.
        let nested_code = kind.ends_with("substitution")
            || kind == "interpolation"
            || kind == "jsx_expression"
            || kind == "jsx_attribute";
        if nested_code {
            return Some(SyntaxContext::Code);
        }
        if kind.starts_with("jsx_") {
            return Some(SyntaxContext::Markup);
        }
        if kind.contains("comment") && start < offset && (offset < end || !closes(syntax, end)) {
            return Some(SyntaxContext::Comment);
        }
        let quoted = kind.contains("string")
            || kind.ends_with("char_literal")
            || kind == "quoted_attribute_value";
        if quoted && start < offset && offset < end {
            return Some(SyntaxContext::String);
        }
        node = current.parent();
    }
    Some(SyntaxContext::Code)
}

/// The buffer's grammar and its own tree edited to follow `text`, none before the first
/// parse, with whether that tree is a parse of `text` itself.
pub(crate) fn tree_for(
    buffer: &Buffer,
    text: &Snapshot,
) -> Option<(&'static Grammar, Option<Tree>, bool)> {
    let syntax = buffer.local::<BufferSyntax>()?;
    let mut tree = syntax.tree.clone();
    let edit = text.edit_from(&syntax.snapshot);
    let parsed = edit.is_none() && is_reparsed(buffer);
    if let (Some(tree), Some(edit)) = (&mut tree, edit) {
        tree.edit(&input_edit(edit, &syntax.snapshot, text));
    }
    Some((syntax.grammar, tree, parsed))
}

/// The innermost code of another language that holds `range` of `text`, overlays aside,
/// its tree edited to follow `text`, with whether that tree is a parse of `text` itself.
pub(crate) fn layer_for(
    buffer: &Buffer,
    text: &Snapshot,
    range: Range<usize>,
) -> Option<(Layer, bool)> {
    let syntax = buffer.local::<BufferSyntax>()?;
    let edit = text.edit_from(&syntax.snapshot);
    let before = |offset: usize| match edit {
        Some(edit) if offset >= edit.new_end => offset - edit.new_end + edit.old_end,
        Some(edit) if offset > edit.start => edit.start,
        _ => offset,
    };
    let before = before(range.start)..before(range.end);
    let outer = (syntax.layers.iter())
        .find(|layer| layer_at(std::slice::from_ref(*layer), &before).is_some())?;
    // Only that one is edited: a Rust file has hundreds, one per macro.
    let mut outer = outer.clone();
    if let Some(edit) = edit {
        outer.edit(&input_edit(edit, &syntax.snapshot, text));
    }
    let layer = layer_at(std::slice::from_ref(&outer), &range)?.clone();
    Some((layer, edit.is_none() && is_reparsed(buffer)))
}

/// The trees of the code of other languages that overlaps `range`, and of that injected
/// into it, overlays aside, as highlighting reads them.
pub(crate) fn layers_in<'a>(
    buffer: &'a Buffer,
    range: &Range<usize>,
) -> Vec<(&'static Grammar, &'a Tree)> {
    fn collect<'a>(
        layers: &'a [Layer],
        range: &Range<usize>,
        found: &mut Vec<(&'static Grammar, &'a Tree)>,
    ) {
        for layer in layers
            .iter()
            .filter(|layer| !layer.overlay && overlaps(&layer.bounds(), range))
        {
            found.push((layer.grammar, &layer.tree));
            collect(&layer.layers, range, found);
        }
    }
    let mut found = Vec::new();
    if let Some(syntax) = buffer.local::<BufferSyntax>() {
        collect(&syntax.layers, range, &mut found);
    }
    found
}

/// Whether the trees are a parse of the text as it is, not ones edited to follow it.
pub(crate) fn is_reparsed(buffer: &Buffer) -> bool {
    current(buffer).is_some_and(|syntax| {
        syntax.tree.is_some()
            && syntax
                .layers_snapshot
                .as_ref()
                .is_some_and(|parsed| parsed.is_same(&syntax.snapshot))
    })
}

pub(crate) fn is_parsed(buffer: &Buffer) -> bool {
    buffer
        .local::<BufferSyntax>()
        .is_some_and(|syntax| syntax.tree.is_some())
}

/// Whether a comment ending at `end` closes there (`*/`, `-->`), unlike a line comment,
/// which point at its end is still in.
fn closes(syntax: &BufferSyntax, end: usize) -> bool {
    let tail = syntax.snapshot.bytes_in(end.saturating_sub(2)..end);
    tail == b"*/" || tail == b"->"
}

fn input_edit(edit: Edit, old: &Snapshot, new: &Snapshot) -> InputEdit {
    let point = |text: &Snapshot, offset| {
        let (row, column) = text.position(offset);
        Point { row, column }
    };
    InputEdit {
        start_byte: edit.start,
        old_end_byte: edit.old_end,
        new_end_byte: edit.new_end,
        start_position: point(new, edit.start),
        old_end_position: point(old, edit.old_end),
        new_end_position: point(new, edit.new_end),
    }
}
