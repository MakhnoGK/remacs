use std::ops::Range;

use remacs_core::{Documentation, Editor, SymbolKind};

use crate::session;

/// Why candidates are wanted, like LSP's `CompletionTriggerKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// Asked for with `completion-at-point`.
    Explicit,
    /// A character was typed; a backend answers for word characters and for the
    /// characters its language completes after, such as `.`.
    Typed(char),
    /// The word grew while the last list was marked incomplete.
    Incomplete,
}

/// What the plugin asks of the window buffer's backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Candidates for the word from `start` to `offset`, where point is.
    Complete {
        id: u64,
        start: usize,
        offset: usize,
        trigger: Trigger,
    },
    /// More about item `index` of list `id`, such as its documentation.
    Resolve { id: u64, index: usize },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub kind: Option<SymbolKind>,
    /// The color the item names, `0xRRGGBBAA`, like Tailwind's `bg-red-500`.
    pub swatch: Option<u32>,
    /// Shown at the end of the row, e.g. a type.
    pub detail: String,
    /// What the text from `start` to point is matched against.
    pub filter_text: String,
    /// The text from `start` to point is replaced with `insert`. It may start before the
    /// word, e.g. a Tailwind class at `bg-` of `bg-re`.
    pub start: usize,
    pub insert: String,
    /// Where point ends up in `insert`; at its end when `None`.
    pub cursor: Option<usize>,
    /// Edits before `start`, e.g. an import, in offsets of the text the list was made for.
    pub extra_edits: Vec<(Range<usize>, String)>,
    pub documentation: Option<Documentation>,
}

pub struct List {
    pub items: Vec<Item>,
    /// Typing more may bring other items, so the list is asked for again.
    pub incomplete: bool,
}

/// Characters that make up the word being completed.
pub fn is_word_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '$'
}

/// What the plugin wants from the window buffer's backend since the last call.
pub fn take_requests(editor: &mut Editor) -> Vec<Request> {
    session::take_requests(editor)
}

/// Answers request `id`; an answer to anything but the newest request is dropped.
pub fn deliver(editor: &mut Editor, id: u64, list: List) {
    session::deliver(editor, id, list);
}

/// Replaces item `index` of list `id` with what the backend found out about it.
pub fn resolved(editor: &mut Editor, id: u64, index: usize, item: Item) {
    session::resolved(editor, id, index, item);
}
