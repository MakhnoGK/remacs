use std::ops::Range;
use std::ptr;
use std::sync::Arc;

use remacs_core::{Buffer, Editor, Span};
use remacs_text::{Edit, Snapshot};
use tree_sitter::{InputEdit, Point, Tree};

use crate::Grammar;
use crate::highlighter::Highlighter;
use crate::parser_thread::{ParseSlot, ParserThread, Request};

/// Larger texts stay unparsed: a full parse would take seconds and its tree hundreds of MiB.
const MAX_PARSED_BYTES: usize = 16 << 20;

/// A buffer's syntax tree. Edits are applied to it as they happen, so its positions match
/// the text at once; the parser thread then replaces it with a fresh parse.
struct BufferSyntax {
    grammar: &'static Grammar,
    /// The text `tree` matches.
    snapshot: Snapshot,
    tree: Option<Tree>,
    highlighter: Option<Arc<Highlighter>>,
    slot: ParseSlot,
}

impl BufferSyntax {
    fn new(grammar: &'static Grammar, snapshot: Snapshot) -> Self {
        Self {
            grammar,
            snapshot,
            tree: None,
            highlighter: None,
            slot: ParseSlot::default(),
        }
    }

    fn request(&self) -> Request {
        Request {
            grammar: self.grammar,
            snapshot: self.snapshot.clone(),
            tree: self.tree.clone(),
            slot: Arc::clone(&self.slot),
        }
    }

    /// Takes the parser thread's result and catches up with `text`; returns a request
    /// when the text changed and needs parsing again.
    fn update(&mut self, text: Snapshot) -> Option<Request> {
        self.take_parse();
        let edit = text.edit_from(&self.snapshot);
        if let (Some(edit), Some(tree)) = (edit, &mut self.tree) {
            tree.edit(&input_edit(edit, &self.snapshot, &text));
        }
        self.snapshot = text;
        edit.map(|_| self.request())
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
        let mut tree = parse.tree;
        if let Some(edit) = self.snapshot.edit_from(&parse.snapshot) {
            tree.edit(&input_edit(edit, &parse.snapshot, &self.snapshot));
        }
        self.tree = Some(tree);
        self.highlighter = parse.highlighter;
    }
}

/// Keeps the window buffer's tree current; runs after every command.
pub(crate) fn sync(editor: &mut Editor, grammars: &'static [Grammar]) {
    let buffer = editor.window_buffer_mut();
    let grammar = grammars
        .iter()
        .find(|grammar| grammar.mode == buffer.mode())
        .filter(|_| buffer.text().len() <= MAX_PARSED_BYTES);
    let Some(grammar) = grammar else {
        buffer.remove_local::<BufferSyntax>();
        return;
    };
    let text = buffer.text().snapshot();
    let request = match buffer.local_mut::<BufferSyntax>() {
        Some(syntax) if ptr::eq(syntax.grammar, grammar) => syntax.update(text),
        _ => Some(buffer.set_local(BufferSyntax::new(grammar, text)).request()),
    };
    if let Some(request) = request {
        let notifier = editor.notifier();
        editor
            .plugin_state::<ParserThread>()
            .send(request, notifier);
    }
}

pub(crate) fn highlight(buffer: &Buffer, range: Range<usize>) -> Vec<Span> {
    let Some(syntax) = buffer.local::<BufferSyntax>() else {
        return Vec::new();
    };
    match (&syntax.tree, &syntax.highlighter) {
        (Some(tree), Some(highlighter)) => highlighter.spans(tree, &syntax.snapshot, range),
        _ => Vec::new(),
    }
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
