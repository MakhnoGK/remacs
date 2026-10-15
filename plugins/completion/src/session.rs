use std::collections::HashSet;
use std::mem;
use std::ops::Range;

use remacs_core::{
    Buffer, Candidate, CommandResult, Editor, Popup, SELF_INSERT_COMMAND, Selection, flex_match,
    flex_rank,
};

use crate::backend::{Item, List, Request, Trigger, is_word_char};

/// Rows shown at once, like `corfu-count`.
const VISIBLE_ROWS: usize = 8;
/// How far back the word at point is looked for; minified lines can be megabytes long.
const MAX_WORD_BYTES: usize = 256;
const DELETE_BACKWARD_CHAR: &str = "delete-backward-char";
const HINTS: &[(&str, &str)] = &[("↵", "Insert"), ("C-n C-p", "Next / previous")];

pub(crate) const COMPLETION_AT_POINT: &str = "completion-at-point";
pub(crate) const NEXT: &str = "completion-next";
pub(crate) const PREVIOUS: &str = "completion-previous";
pub(crate) const INSERT: &str = "completion-insert";
pub(crate) const QUIT: &str = "completion-quit";
pub(crate) const SCROLL_DOCUMENTATION: &str = "completion-scroll-documentation";
pub(crate) const SCROLL_DOCUMENTATION_DOWN: &str = "completion-scroll-documentation-down";
/// They update the popup themselves.
const OWN_COMMANDS: &[&str] = &[
    COMPLETION_AT_POINT,
    NEXT,
    PREVIOUS,
    INSERT,
    QUIT,
    SCROLL_DOCUMENTATION,
    SCROLL_DOCUMENTATION_DOWN,
];

#[derive(Default)]
struct Completion {
    next_id: u64,
    awaited: Option<Awaited>,
    requests: Vec<Request>,
    session: Option<Session>,
    /// `Editor::command_count` when last looked at.
    seen_commands: u64,
}

/// The newest list asked for.
#[derive(Clone, Copy)]
struct Awaited {
    id: u64,
    start: usize,
    offset: usize,
    trigger: Trigger,
}

/// A list being completed from, like a corfu session: it lasts while the word at point
/// is typed or erased and ends with any other command.
struct Session {
    id: u64,
    start: usize,
    items: Vec<Item>,
    incomplete: bool,
    /// Indices of the items matching the word, prefix matches first.
    matches: Vec<usize>,
    /// The word matched; the rows shown highlight what of it their labels match.
    word: String,
    selection: Selection,
    /// Point when the word was matched; moving it other than by typing ends the session.
    point: usize,
    resolving: HashSet<usize>,
    /// Pages the selected item's documentation was scrolled by.
    documentation_pages: i32,
}

impl Session {
    fn new(id: u64, start: usize, list: List) -> Self {
        Self {
            id,
            start,
            items: list.items,
            incomplete: list.incomplete,
            matches: Vec::new(),
            word: String::new(),
            selection: Selection::default(),
            point: start,
            resolving: HashSet::new(),
            documentation_pages: 0,
        }
    }

    fn filter(&mut self, word: &str, point: usize) {
        let mut ranked: Vec<_> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let is_prefix = flex_rank(word, &item.filter_text)?;
                Some((!is_prefix, index))
            })
            .collect();
        ranked.sort_unstable();
        self.matches = ranked.into_iter().map(|(_, index)| index).collect();
        word.clone_into(&mut self.word);
        self.selection = Selection::default();
        self.documentation_pages = 0;
        self.point = point;
    }

    fn is_shown(&self) -> bool {
        !self.matches.is_empty()
    }

    fn select(&mut self, index: usize) {
        self.selection
            .select(index, self.matches.len(), VISIBLE_ROWS);
        self.documentation_pages = 0;
    }

    fn selected_item(&self) -> Option<&Item> {
        let index = self.matches.get(self.selection.selected())?;
        Some(&self.items[*index])
    }
}

impl Completion {
    fn request(&mut self, start: usize, offset: usize, trigger: Trigger) {
        self.next_id += 1;
        let id = self.next_id;
        self.awaited = Some(Awaited {
            id,
            start,
            offset,
            trigger,
        });
        self.requests.push(Request::Complete {
            id,
            start,
            offset,
            trigger,
        });
    }

    /// Asks once for what the selected item lacks, such as its documentation.
    fn resolve_selected(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        let Some(&index) = session.matches.get(session.selection.selected()) else {
            return;
        };
        if session.items[index].documentation.is_none() && session.resolving.insert(index) {
            self.requests.push(Request::Resolve {
                id: session.id,
                index,
            });
        }
    }

    fn close(&mut self) {
        self.session = None;
        self.awaited = None;
    }
}

fn completion(editor: &mut Editor) -> &mut Completion {
    editor.plugin_state()
}

/// Opens, narrows or closes the popup after each command, like corfu's post-command hook.
pub(crate) fn update(editor: &mut Editor) {
    let count = editor.command_count();
    if mem::replace(&mut completion(editor).seen_commands, count) == count {
        return;
    }
    let command = editor.last_command();
    if command.is_some_and(|command| OWN_COMMANDS.contains(&command)) {
        return;
    }
    let typed = editor.minibuffer().is_none()
        && matches!(command, Some(SELF_INSERT_COMMAND | DELETE_BACKWARD_CHAR));
    if !typed {
        completion(editor).close();
        return;
    }
    if completion(editor).session.is_some() && refilter(editor) {
        let point = editor.window_buffer().point();
        let state = completion(editor);
        let incomplete = state
            .session
            .as_ref()
            .filter(|session| session.incomplete)
            .map(|session| session.start);
        if let Some(start) = incomplete {
            state.request(start, point, Trigger::Incomplete);
        }
        state.resolve_selected();
        return;
    }
    if command == Some(SELF_INSERT_COMMAND) {
        request_after_typing(editor);
    }
}

fn request_after_typing(editor: &mut Editor) {
    let buffer = editor.window_buffer();
    let point = buffer.point();
    let Some(typed) = buffer.char_before(point) else {
        return;
    };
    let start = word_start(buffer, point);
    if is_word_char(typed)
        && buffer
            .char_after(start)
            .is_some_and(|first| first.is_ascii_digit())
    {
        return;
    }
    let state = completion(editor);
    // The list on its way is narrowed to the longer word when it comes.
    if is_word_char(typed) && state.awaited.is_some_and(|awaited| awaited.start == start) {
        return;
    }
    state.request(start, point, Trigger::Typed(typed));
}

/// Matches the items against the word typed so far; false when point has left the
/// word, which ends the session. A complete list that no longer matches ends it too.
fn refilter(editor: &mut Editor) -> bool {
    let Some(start) = editor
        .find_plugin_state::<Completion>()
        .and_then(|state| state.session.as_ref())
        .map(|session| session.start)
    else {
        return false;
    };
    let buffer = editor.window_buffer();
    let point = buffer.point();
    let word = (point >= start)
        .then(|| buffer.contents(start..point))
        .filter(|word| word.chars().all(is_word_char));
    let state = completion(editor);
    let (Some(word), Some(session)) = (word, &mut state.session) else {
        state.close();
        return false;
    };
    session.filter(&word, point);
    if !session.is_shown() && !session.incomplete {
        state.session = None;
    }
    true
}

/// Where the word ending at `point` starts.
fn word_start(buffer: &Buffer, point: usize) -> usize {
    let text = buffer.text();
    let line_start = text.line_start(text.line_of(point)).unwrap_or(0);
    let mut from = line_start.max(point.saturating_sub(MAX_WORD_BYTES));
    if !text.is_char_boundary(from) {
        from = text.next_char_boundary(from);
    }
    let word_len: usize = buffer
        .contents(from..point)
        .chars()
        .rev()
        .take_while(|&character| is_word_char(character))
        .map(char::len_utf8)
        .sum();
    point - word_len
}

pub(crate) fn take_requests(editor: &mut Editor) -> Vec<Request> {
    mem::take(&mut completion(editor).requests)
}

pub(crate) fn deliver(editor: &mut Editor, id: u64, list: List) {
    let state = completion(editor);
    let Some(awaited) = state.awaited.filter(|awaited| awaited.id == id) else {
        return;
    };
    state.awaited = None;
    state.session = Some(Session::new(id, awaited.start, list));
    if !refilter(editor) {
        return;
    }
    let point = editor.window_buffer().point();
    let state = completion(editor);
    let Some(incomplete) = state.session.as_ref().map(|session| session.incomplete) else {
        if awaited.trigger == Trigger::Explicit {
            editor.set_message("No completions");
        }
        return;
    };
    if incomplete && point != awaited.offset {
        state.request(awaited.start, point, Trigger::Incomplete);
    }
    state.resolve_selected();
}

pub(crate) fn resolved(editor: &mut Editor, id: u64, index: usize, item: Item) {
    let session = completion(editor)
        .session
        .as_mut()
        .filter(|session| session.id == id);
    if let Some(slot) = session.and_then(|session| session.items.get_mut(index)) {
        *slot = item;
    }
}

/// Point moved by the mouse hides the popup; typing on in the word brings it back.
fn shown_session(editor: &Editor) -> Option<&Session> {
    if editor.minibuffer().is_some() {
        return None;
    }
    let point = editor.window_buffer().point();
    editor
        .find_plugin_state::<Completion>()?
        .session
        .as_ref()
        .filter(|session| session.is_shown() && session.point == point)
}

/// Whether the popup's keys are active.
pub(crate) fn is_shown(editor: &Editor) -> bool {
    shown_session(editor).is_some()
}

pub(crate) fn popup(editor: &Editor) -> Option<Popup> {
    let session = shown_session(editor)?;
    let visible = session
        .selection
        .visible(session.matches.len(), VISIBLE_ROWS);
    let rows = session.matches[visible.clone()]
        .iter()
        .map(|&index| {
            let item = &session.items[index];
            Candidate {
                matches: flex_match(&session.word, &item.label).unwrap_or_default(),
                detail: (!item.detail.is_empty()).then(|| item.detail.clone().into()),
                kind: item.kind,
                ..Candidate::new(item.label.clone())
            }
        })
        .collect();
    let documentation = session
        .selected_item()
        .and_then(|item| item.documentation.clone())
        .filter(|documentation| !documentation.is_empty());
    Some(Popup {
        anchor: session.start,
        rows,
        selected: Some(session.selection.selected() - visible.start),
        documentation,
        documentation_pages: session.documentation_pages,
        hints: HINTS,
        notes: Vec::new(),
    })
}

pub(crate) fn completion_at_point(editor: &mut Editor) -> CommandResult {
    if editor.minibuffer().is_some() {
        return Err("Completion at point works in buffers only".into());
    }
    let buffer = editor.window_buffer();
    let point = buffer.point();
    let start = word_start(buffer, point);
    let state = completion(editor);
    state.close();
    state.request(start, point, Trigger::Explicit);
    Ok(())
}

pub(crate) fn next(editor: &mut Editor) -> CommandResult {
    step(editor, true)
}

pub(crate) fn previous(editor: &mut Editor) -> CommandResult {
    step(editor, false)
}

fn step(editor: &mut Editor, forward: bool) -> CommandResult {
    let state = completion(editor);
    let session = state.session.as_mut().ok_or("No completion")?;
    let selected = session.selection.selected();
    session.select(if forward {
        selected + 1
    } else {
        selected.saturating_sub(1)
    });
    state.resolve_selected();
    Ok(())
}

/// Scrolls the selected item's documentation a page forward, like `scroll-other-window`.
pub(crate) fn scroll_documentation(editor: &mut Editor) -> CommandResult {
    scroll_documentation_by(editor, 1)
}

pub(crate) fn scroll_documentation_down(editor: &mut Editor) -> CommandResult {
    scroll_documentation_by(editor, -1)
}

fn scroll_documentation_by(editor: &mut Editor, pages: i32) -> CommandResult {
    let session = completion(editor).session.as_mut().ok_or("No completion")?;
    session.documentation_pages += pages;
    Ok(())
}

pub(crate) fn quit(editor: &mut Editor) -> CommandResult {
    completion(editor).close();
    Ok(())
}

/// Replaces the word with the selected item and makes its edits elsewhere, e.g. an import.
pub(crate) fn insert(editor: &mut Editor) -> CommandResult {
    let state = completion(editor);
    let session = state.session.take().ok_or("No completion")?;
    state.awaited = None;
    let &index = session
        .matches
        .get(session.selection.selected())
        .ok_or("No completion")?;
    let mut items = session.items;
    let mut item = items.swap_remove(index);
    let buffer = editor.window_buffer_mut();
    let point = buffer.point();
    let start = item.start.min(point);
    buffer.delete(start..point);
    buffer.insert(&item.insert);
    if let Some(cursor) = item.cursor {
        buffer.set_point(start + cursor);
    }
    item.extra_edits.retain(|(range, _)| range.end <= start);
    item.extra_edits
        .sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, text) in &item.extra_edits {
        replace_keeping_point(buffer, range.clone(), text);
    }
    Ok(())
}

/// Replaces `range`, which ends before point, and keeps point on its character.
fn replace_keeping_point(buffer: &mut Buffer, range: Range<usize>, text: &str) {
    buffer.delete(range.clone());
    let point = buffer.point();
    buffer.set_point(range.start);
    buffer.insert(text);
    buffer.set_point(point + text.len());
}
