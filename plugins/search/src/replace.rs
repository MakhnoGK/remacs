//! Replacing text from point on, asking at each match, like Emacs `query-replace`.

use std::ops::Range;

use remacs_core::{Buffer, CommandResult, Completions, Editor, Keymap, Prompt};

use crate::preview::{self, Lit};
use crate::query::Query;

pub const QUERY_REPLACE: &str = "query-replace";
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
    let prompt = Prompt {
        label: "Replace".into(),
        category: FROM_CATEGORY,
        placeholder: "text",
        hints: &[("↵", "Next"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |editor, input| count_matches(editor.window_buffer(), input),
        |editor, from| {
            preview::end(editor);
            let query = Query::new(&from).ok_or("Nothing to replace")?;
            let prompt = Prompt {
                label: format!("Replace “{from}” with"),
                category: TO_CATEGORY,
                placeholder: "text",
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
    preview::begin(editor, FROM_CATEGORY);
    Ok(())
}

pub fn replace_match(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    let replacing = buffer.local_mut::<Replacing>().ok_or(NOT_REPLACING)?;
    let (current, to) = (replacing.current.clone(), replacing.to.clone());
    replacing.replaced += 1;
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
    let (query, from, to) = (
        replacing.query.clone(),
        replacing.current.start,
        replacing.to.clone(),
    );
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
    let current = next_match(buffer, &query, buffer.point())
        .ok_or_else(|| format!("No “{from}” after point"))?;
    // Every replacement then undoes at once.
    buffer.add_undo_boundary();
    buffer.set_local(Replacing {
        query,
        to: to.clone(),
        current,
        replaced: 0,
    });
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
        title: Some((format!("Replace “{from}” with “{to}”"), String::new())),
        keymap: Some(keymap),
        hints: &[
            ("y", "Replace"),
            ("n", "Skip"),
            ("!", "Replace all"),
            ("q", "Done"),
        ],
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

/// On to the next match from `offset`, else done.
fn advance(editor: &mut Editor, offset: usize) {
    let buffer = editor.window_buffer_mut();
    let next = buffer
        .local::<Replacing>()
        .and_then(|replacing| next_match(buffer, &replacing.query, offset));
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
    let (query, current) = (replacing.query.clone(), replacing.current.clone());
    let offset = current.start;
    buffer.set_local(Lit {
        query,
        current: Some(current),
    });
    preview::show(editor, offset);
}

fn stop(editor: &mut Editor) {
    editor.cancel_minibuffer();
    finish(editor);
}

fn finish(editor: &mut Editor) {
    let buffer = editor.window_buffer_mut();
    let Some(replaced) = buffer
        .local::<Replacing>()
        .map(|replacing| replacing.replaced)
    else {
        return;
    };
    buffer.remove_local::<Replacing>();
    buffer.remove_local::<Lit>();
    buffer.add_undo_boundary();
    let noun = if replaced == 1 {
        "occurrence"
    } else {
        "occurrences"
    };
    editor.set_message(format!("Replaced {replaced} {noun}"));
}

fn replacing(buffer: &Buffer) -> Result<&Replacing, &'static str> {
    buffer.local::<Replacing>().ok_or(NOT_REPLACING)
}

/// Replaces every match from `from` on with `to`, as one edit; returns how many.
pub fn replace_from(buffer: &mut Buffer, query: &Query, from: usize, to: &str) -> usize {
    let mut matches = Vec::new();
    let len = query.match_len();
    query.find(buffer.text(), from..buffer.text().len(), |start| {
        matches.push(start..start + len);
        true
    });
    let count = matches.len();
    buffer.apply_edits(
        matches
            .into_iter()
            .map(|range| (range, to.to_owned()))
            .collect(),
    );
    count
}

pub fn next_match(buffer: &Buffer, query: &Query, from: usize) -> Option<Range<usize>> {
    let text = buffer.text();
    let mut found = None;
    query.find(text, from.min(text.len())..text.len(), |start| {
        found = Some(start..start + query.match_len());
        false
    });
    found
}

/// How many times `input` occurs, said in place of the count.
fn count_matches(buffer: &Buffer, input: &str) -> Completions {
    let Some(query) = Query::new(input) else {
        return Completions::default();
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
    Completions {
        note: Some(note),
        ..Completions::default()
    }
}
