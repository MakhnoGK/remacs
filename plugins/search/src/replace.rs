//! Replacing text from point on, asking at each match, like Emacs `query-replace`.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use remacs_core::{AnswerCheck, Buffer, CommandResult, Completions, Editor, Keymap, Prompt, quote};

use crate::preview::{self, Lit};
use crate::query::{self, Query, regexp_option};

pub const QUERY_REPLACE: &str = "query-replace";
pub const QUERY_REPLACE_REGEXP: &str = "query-replace-regexp";
pub const REPLACE_MATCH: &str = "query-replace-match";
pub const SKIP_MATCH: &str = "query-replace-skip";
pub const REPLACE_ALL: &str = "query-replace-all";
pub const QUIT: &str = "query-replace-quit";
const FROM_CATEGORY: &str = "replace";
const TO_CATEGORY: &str = "replace-with";
const QUERY_CATEGORY: &str = "replace-query";
const NOT_REPLACING: &str = "Not replacing";
/// Matches counted while the text to replace is typed, at most.
const MAX_COUNTED: usize = 10_000;

/// Kept on the buffer while its matches are asked about.
struct Replacing {
    query: Query,
    to: String,
    current: Range<usize>,
    replaced: usize,
}

/// Reads the text to replace, its matches lit as it is typed, then what replaces it, and
/// asks at each match after point.
pub fn query_replace(editor: &mut Editor) -> CommandResult {
    query_replace_text(editor, false)
}

/// Like [`query_replace`] for a regular expression, its groups put in the replacement by
/// `$1` or `${name}`.
pub fn query_replace_regexp(editor: &mut Editor) -> CommandResult {
    query_replace_text(editor, true)
}

fn query_replace_text(editor: &mut Editor, regex: bool) -> CommandResult {
    let regex = Rc::new(Cell::new(regex));
    let counted_regex = Rc::clone(&regex);
    let answered_regex = Rc::clone(&regex);
    let prompt = Prompt {
        label: "Replace".into(),
        category: FROM_CATEGORY,
        placeholder: "text",
        hints: &[("↵", "Next"), ("C-g", "Cancel")],
        options: vec![regexp_option(&regex)],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        move |editor, input| count_matches(editor.window_buffer(), input, counted_regex.get()),
        move |editor, from| {
            preview::end(editor);
            let regex = answered_regex.get();
            let query = Query::new(&from, regex)?.ok_or("Nothing to replace")?;
            let prompt = Prompt {
                label: format!("Replace{} “{}” with", regexp_word(regex), quote(&from)),
                category: TO_CATEGORY,
                placeholder: replacement_placeholder(regex),
                check: Some(replacement_check(&query)),
                hints: &[("↵", "Replace…"), ("C-g", "Cancel")],
                ..Prompt::default()
            };
            editor.read(
                prompt,
                |_, _| Completions::default(),
                move |editor, to| ask(editor, query, from, to),
            )
        },
    )?;
    preview::begin(editor, FROM_CATEGORY, &regex);
    Ok(())
}

/// Said after "Replace" for a regular expression.
pub fn regexp_word(regex: bool) -> &'static str {
    if regex { " regexp" } else { "" }
}

/// Keeps the replacement's prompt open while it names a group the regexp lacks.
pub fn replacement_check(query: &Query) -> AnswerCheck {
    let query = query.clone();
    Rc::new(move |to| query.check_replacement(to))
}

pub fn replacement_placeholder(regex: bool) -> &'static str {
    if regex { "text, $1 for a group" } else { "text" }
}

pub fn replace_match(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    let replacing = replacing(buffer)?;
    let current = replacing.current.clone();
    let to = replacing.query.replacement(buffer.text(), current.clone(), &replacing.to);
    if let Some(replacing) = buffer.local_mut::<Replacing>() {
        replacing.replaced += 1;
    }
    buffer.apply_edits(vec![(current.clone(), to.clone())]);
    advance(editor, current.start + to.len());
    Ok(())
}

pub fn skip_match(editor: &mut Editor) -> CommandResult {
    let current = replacing(editor.window_buffer())?.current.clone();
    advance(editor, current.end);
    Ok(())
}

/// Replaces this match and the rest at once, as one edit.
pub fn replace_all(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    let replacing = replacing(buffer)?;
    let (query, from, to) =
        (replacing.query.clone(), replacing.current.start, replacing.to.clone());
    let count = replace_from(buffer, &query, from, &to);
    if let Some(replacing) = buffer.local_mut::<Replacing>() {
        replacing.replaced += count;
    }
    stop(editor);
    Ok(())
}

pub fn quit(editor: &mut Editor) -> CommandResult {
    replacing(editor.window_buffer())?;
    stop(editor);
    Ok(())
}

/// Ends the replacing once its prompt was quit. Runs after every command.
pub fn sync(editor: &mut Editor) {
    let asking = editor
        .minibuffer()
        .is_some_and(|minibuffer| minibuffer.prompt().category == QUERY_CATEGORY);
    if !asking && editor.window_buffer().local::<Replacing>().is_some() {
        finish(editor);
    }
}

fn ask(editor: &mut Editor, query: Query, from: String, to: String) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    let current = next_match(buffer, &query, buffer.point(), false)
        .ok_or_else(|| format!("No “{from}” after point"))?;
    // Every replacement then undoes at once.
    buffer.add_undo_boundary();
    buffer.set_local(Replacing { query, to: to.clone(), current, replaced: 0 });
    show_current(editor);
    let mut keymap = Keymap::default();
    for (keys, command) in [
        ("y", REPLACE_MATCH),
        ("SPC", REPLACE_MATCH),
        ("n", SKIP_MATCH),
        ("DEL", SKIP_MATCH),
        ("!", REPLACE_ALL),
        ("q", QUIT),
    ] {
        keymap.bind(keys, command);
    }
    let prompt = Prompt {
        category: QUERY_CATEGORY,
        read_only: true,
        title: Some((format!("Replace “{}” with “{}”", quote(&from), quote(&to)), String::new())),
        keymap: Some(keymap),
        hints: &[("y", "Replace"), ("n", "Skip"), ("!", "Replace all"), ("q", "Done")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        |editor, _| {
            finish(editor);
            Ok(())
        },
    )
}

/// On to the next match from `offset`, where the last one ended, else done.
fn advance(editor: &mut Editor, offset: usize) {
    let buffer = editor.window_buffer_mut();
    let next = buffer
        .local::<Replacing>()
        .and_then(|replacing| next_match(buffer, &replacing.query, offset, true));
    match (next, buffer.local_mut::<Replacing>()) {
        (Some(next), Some(replacing)) => {
            replacing.current = next;
            show_current(editor);
        }
        _ => stop(editor),
    }
}

fn show_current(editor: &mut Editor) {
    let buffer = editor.window_buffer_mut();
    let Some(replacing) = buffer.local::<Replacing>() else {
        return;
    };
    let (query, offset) = (replacing.query.clone(), replacing.current.start);
    buffer.set_local(Lit { query, current: Some(offset) });
    preview::show(editor, offset);
}

fn stop(editor: &mut Editor) {
    editor.cancel_minibuffer();
    finish(editor);
}

fn finish(editor: &mut Editor) {
    let buffer = editor.window_buffer_mut();
    let Some(replaced) = buffer.local::<Replacing>().map(|replacing| replacing.replaced) else {
        return;
    };
    buffer.remove_local::<Replacing>();
    buffer.remove_local::<Lit>();
    buffer.add_undo_boundary();
    let noun = if replaced == 1 { "occurrence" } else { "occurrences" };
    editor.set_done(format!("Replaced {replaced} {noun}"));
}

fn replacing(buffer: &Buffer) -> Result<&Replacing, &'static str> {
    buffer.local::<Replacing>().ok_or(NOT_REPLACING)
}

/// Replaces every match from `from` on with `to`, as one edit; returns how many.
pub fn replace_from(buffer: &mut Buffer, query: &Query, from: usize, to: &str) -> usize {
    let text = buffer.text();
    let mut edits = Vec::new();
    query.find(text, from..text.len(), |matched| {
        edits.push((matched.clone(), query.replacement(text, matched, to)));
        true
    });
    let count = edits.len();
    buffer.apply_edits(edits);
    count
}

/// The first match from `from`. Right after a match, `from` being its end, an empty match
/// there is passed over, as it would be found again and again.
pub fn next_match(
    buffer: &Buffer,
    query: &Query,
    from: usize,
    after_match: bool,
) -> Option<Range<usize>> {
    let text = buffer.text();
    let mut found = None;
    query.find(text, from.min(text.len())..text.len(), |matched| {
        if after_match && matched == (from..from) {
            return true;
        }
        found = Some(matched);
        false
    });
    found
}

/// How many times `input` occurs, said in place of the count.
fn count_matches(buffer: &Buffer, input: &str, regex: bool) -> Completions {
    let query = match Query::parse(input, regex) {
        Ok(query) => query,
        Err(completions) => return *completions,
    };
    let text = buffer.text();
    let mut count = 0;
    query.find(text, 0..text.len(), |_| {
        count += 1;
        count <= MAX_COUNTED
    });
    let note = match count {
        0 => "No matches".to_owned(),
        1 => "1 match".to_owned(),
        _ if count > MAX_COUNTED => format!("Over {MAX_COUNTED} matches"),
        _ => format!("{count} matches"),
    };
    query::note(note)
}
