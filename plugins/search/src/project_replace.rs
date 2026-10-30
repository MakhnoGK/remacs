//! Replacing text across the whole project, asking at each match, like Emacs
//! `project-query-replace-regexp` with literal text: the files that have it are found in
//! the background and shown in turn, and each one changed is saved.

use std::collections::HashSet;
use std::mem;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use remacs_core::{CommandResult, Completions, Editor, Keymap, Prompt, quote};
use remacs_files::{FileIndex, FilePreview};

use crate::preview::{self, Lit};
use crate::project::{read_lines, searched, unsaved_buffers};
use crate::query::Query;
use crate::replace::{
    next_match, regexp_word, replace_from, replacement_check, replacement_placeholder,
};
use crate::scan::{Job, Scan};
use crate::words::Words;

pub const PROJECT_QUERY_REPLACE: &str = "project-query-replace";
pub const REPLACE_MATCH: &str = "project-replace-match";
pub const SKIP_MATCH: &str = "project-replace-skip";
pub const REPLACE_FILE: &str = "project-replace-file";
pub const REPLACE_ALL: &str = "project-replace-all";
pub const QUIT: &str = "project-replace-quit";
const FROM_CATEGORY: &str = "project-replace";
/// Shared with `query-replace`, so either offers the replacements typed in the other.
const TO_CATEGORY: &str = "replace-with";
const ASK_CATEGORY: &str = "project-replace-ask";
const NOT_REPLACING: &str = "Not replacing in the project";

#[derive(Default)]
struct ProjectReplace {
    replacing: Option<Replacing>,
}

struct Replacing {
    root: PathBuf,
    index: Arc<FileIndex>,
    words: Arc<Words>,
    query: Query,
    to: String,
    /// Finds the files that have the text, once the project's files are all listed.
    scan: Option<Scan>,
    /// Files taken from the scan, by their place in its list.
    taken: HashSet<usize>,
    /// The file shown and the match asked about.
    current: Option<(PathBuf, Range<usize>)>,
    replaced_in_file: usize,
    /// The rest is replaced without asking.
    everywhere: bool,
    replaced: usize,
    files_saved: usize,
    errors: Vec<String>,
    preview: FilePreview,
}

/// What [`next_file`] found.
enum Next {
    File,
    /// The search has not found another file yet.
    Wait,
    Done,
}

/// Reads the text to replace, listing the project's lines that have it, then what
/// replaces it, and asks at each match, file by file.
pub fn project_query_replace(editor: &mut Editor) -> CommandResult {
    let prompt = Prompt {
        label: "Replace in project".into(),
        category: FROM_CATEGORY,
        // The answer is the line selected, not the text typed.
        private: true,
        placeholder: "Text to replace…",
        hints: &[("↵", "Next"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    read_lines(editor, prompt, true, |editor, _| {
        let (root, index, from, regex) = searched(editor).ok_or("Nothing to replace")?;
        let words = match regex {
            true => Words::regex(&from)?,
            false => Words::phrase(&from),
        };
        let (query, words) = Query::new(&from, regex)?.zip(words).ok_or("Nothing to replace")?;
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
            move |editor, to| {
                let replacing = Replacing {
                    root,
                    index,
                    words: Arc::new(words),
                    query,
                    to: to.clone(),
                    scan: None,
                    taken: HashSet::new(),
                    current: None,
                    replaced_in_file: 0,
                    everywhere: false,
                    replaced: 0,
                    files_saved: 0,
                    errors: Vec::new(),
                    preview: FilePreview::begin(editor),
                };
                ask(editor, replacing, &from, &to)
            },
        )
    })
}

pub fn replace_match(editor: &mut Editor) -> CommandResult {
    let (range, to) = current(editor)?;
    editor.window_buffer_mut().apply_edits(vec![(range.clone(), to.clone())]);
    if let Some(state) = active(editor) {
        state.replaced_in_file += 1;
    }
    go_on(editor, Some(range.start + to.len()));
    Ok(())
}

pub fn skip_match(editor: &mut Editor) -> CommandResult {
    let (range, _) = current(editor)?;
    go_on(editor, Some(range.end));
    Ok(())
}

/// Replaces this match and the rest of the file, then asks in the next file.
pub fn replace_file(editor: &mut Editor) -> CommandResult {
    let (range, _) = current(editor)?;
    replace_rest(editor, range.start);
    finish_file(editor);
    go_on(editor, None);
    Ok(())
}

/// Replaces this match and every one after it, in every file, without asking.
pub fn replace_all(editor: &mut Editor) -> CommandResult {
    let (range, _) = current(editor)?;
    if let Some(state) = active(editor) {
        state.everywhere = true;
    }
    go_on(editor, Some(range.start));
    Ok(())
}

pub fn quit(editor: &mut Editor) -> CommandResult {
    active(editor).ok_or(NOT_REPLACING)?;
    stop(editor);
    Ok(())
}

/// Goes on once the search finds more files, and ends the replacing once its prompt was
/// quit. Runs after every command.
pub fn update(editor: &mut Editor) {
    let Some(waiting) = active(editor).map(|state| state.current.is_none()) else {
        return;
    };
    let asking =
        editor.minibuffer().is_some_and(|minibuffer| minibuffer.prompt().category == ASK_CATEGORY);
    if !asking {
        finish(editor);
    } else if waiting {
        go_on(editor, None);
    }
}

fn ask(editor: &mut Editor, replacing: Replacing, from: &str, to: &str) -> CommandResult {
    editor.plugin_state::<ProjectReplace>().replacing = Some(replacing);
    let mut keymap = Keymap::default();
    for (keys, command) in [
        ("y", REPLACE_MATCH),
        ("SPC", REPLACE_MATCH),
        ("n", SKIP_MATCH),
        ("DEL", SKIP_MATCH),
        ("!", REPLACE_FILE),
        ("A", REPLACE_ALL),
        ("q", QUIT),
    ] {
        keymap.bind(keys, command);
    }
    let prompt = Prompt {
        category: ASK_CATEGORY,
        read_only: true,
        title: Some((format!("Replace “{}” with “{}”", quote(from), quote(to)), String::new())),
        keymap: Some(keymap),
        hints: &[
            ("y", "Replace"),
            ("n", "Skip"),
            ("!", "Rest of file"),
            ("A", "All files"),
            ("q", "Done"),
        ],
        ..Prompt::default()
    };
    editor.read(prompt, status, |editor, _| {
        finish(editor);
        Ok(())
    })?;
    go_on(editor, None);
    Ok(())
}

/// The file shown, or that the search goes on.
fn status(editor: &Editor, _: &str) -> Completions {
    let state =
        editor.find_plugin_state::<ProjectReplace>().and_then(|state| state.replacing.as_ref());
    let note = state.map(|state| match &state.current {
        Some((path, _)) => relative(path, &state.root),
        None => "Searching…".to_owned(),
    });
    Completions { note, ..Completions::default() }
}

/// Asks at the next match from `offset` in the file shown, else in the next file that has
/// one, replacing without asking once told to; ends when there are none left. With
/// `None`, starts at the next file.
fn go_on(editor: &mut Editor, mut offset: Option<usize>) {
    // The offset is where the last match ended, until the next file is shown.
    let mut after_match = true;
    loop {
        let Some(state) = active(editor) else {
            return;
        };
        let (query, everywhere) = (state.query.clone(), state.everywhere);
        if let Some(from) = offset.take() {
            if everywhere {
                replace_rest(editor, from);
            } else if let Some(range) =
                next_match(editor.window_buffer(), &query, from, after_match)
            {
                show(editor, range);
                editor.refresh_minibuffer();
                return;
            }
            finish_file(editor);
        }
        match next_file(editor) {
            Next::File => {
                offset = Some(0);
                after_match = false;
            }
            Next::Wait => {
                editor.refresh_minibuffer();
                return;
            }
            Next::Done => {
                stop(editor);
                return;
            }
        }
    }
}

/// Shows the next file with the text, its matches lit; starts the search once the
/// project's files are all listed.
fn next_file(editor: &mut Editor) -> Next {
    loop {
        if !start_search(editor) {
            return Next::Wait;
        }
        let Some(state) = active(editor) else {
            return Next::Done;
        };
        let Some(scan) = &state.scan else {
            return Next::Done;
        };
        let (next, done) = {
            let found = scan.found();
            let next = found
                .files
                .iter()
                .find(|(index, _)| !state.taken.contains(index))
                .map(|(&index, (file, _))| (index, file.clone()));
            (next, found.done)
        };
        let Some((index, file)) = next else {
            return if done { Next::Done } else { Next::Wait };
        };
        state.taken.insert(index);
        let path = state.root.join(file);
        let mut preview = mem::take(&mut state.preview);
        let visited = preview.visit(editor, &path);
        let Some(state) = active(editor) else {
            return Next::Done;
        };
        state.preview = preview;
        match visited {
            Ok(()) => {
                state.current = Some((path, 0..0));
                // Every replacement in the file then undoes at once.
                editor.window_buffer_mut().add_undo_boundary();
                return Next::File;
            }
            Err(error) => state.errors.push(format!("{}: {error}", path.display())),
        }
    }
}

/// Starts looking for the files with the text once the project's files are all listed;
/// returns whether it has.
fn start_search(editor: &mut Editor) -> bool {
    let Some(state) = active(editor) else {
        return false;
    };
    if state.scan.is_some() {
        return true;
    }
    let (files, listed) = state.index.files_and_scanned();
    if !listed {
        return false;
    }
    let (root, words) = (state.root.clone(), Arc::clone(&state.words));
    let unsaved = unsaved_buffers(editor, &root);
    let job = Job {
        root,
        files,
        unsaved,
        words,
        max_lines: usize::MAX,
        // Which files have it is enough; each is searched again once shown.
        max_file_lines: 1,
    };
    let scan = Scan::start(job, editor.notifier());
    if let Some(state) = active(editor) {
        state.scan = Some(scan);
    }
    true
}

/// Puts point on `range`, lit apart from the other matches.
fn show(editor: &mut Editor, range: Range<usize>) {
    let Some(state) = active(editor) else {
        return;
    };
    let query = state.query.clone();
    if let Some((_, current)) = &mut state.current {
        *current = range.clone();
    }
    editor.window_buffer_mut().set_local(Lit { query, current: Some(range.start) });
    preview::show(editor, range.start);
}

/// Replaces every match in the file shown from `offset` on.
fn replace_rest(editor: &mut Editor, offset: usize) {
    let Some(state) = active(editor) else {
        return;
    };
    let (query, to) = (state.query.clone(), state.to.clone());
    let count = replace_from(editor.window_buffer_mut(), &query, offset, &to);
    if let Some(state) = active(editor) {
        state.replaced_in_file += count;
    }
}

/// Saves the file shown if anything in it was replaced, and closes it if it was opened
/// to be shown.
fn finish_file(editor: &mut Editor) {
    let Some(state) = active(editor) else {
        return;
    };
    let Some((path, _)) = state.current.take() else {
        return;
    };
    let replaced = mem::take(&mut state.replaced_in_file);
    let mut preview = mem::take(&mut state.preview);
    let saved =
        editor.buffers_mut().iter_mut().find(|buffer| buffer.path() == Some(path.as_path())).map(
            |buffer| {
                buffer.remove_local::<Lit>();
                if replaced == 0 {
                    return Ok(false);
                }
                buffer.add_undo_boundary();
                buffer.save().map(|()| true)
            },
        );
    preview.close(editor, &path);
    let Some(state) = active(editor) else {
        return;
    };
    state.preview = preview;
    state.replaced += replaced;
    match saved {
        Some(Ok(true)) => state.files_saved += 1,
        Some(Err(error)) => state.errors.push(format!("{}: {error}", path.display())),
        _ => {}
    }
}

fn stop(editor: &mut Editor) {
    editor.cancel_minibuffer();
    finish(editor);
}

/// Puts back what the window showed and says what was replaced.
fn finish(editor: &mut Editor) {
    finish_file(editor);
    let Some(state) = editor.plugin_state::<ProjectReplace>().replacing.take() else {
        return;
    };
    state.preview.end(editor, None);
    match state.errors.first() {
        Some(error) => editor.set_error(format!("Could not replace in {error}")),
        None if state.taken.is_empty() => editor.set_message("No matches in the project"),
        None => editor.set_done(format!(
            "Replaced {} in {}",
            plural(state.replaced, "occurrence"),
            plural(state.files_saved, "file")
        )),
    }
}

fn active(editor: &mut Editor) -> Option<&mut Replacing> {
    editor.plugin_state::<ProjectReplace>().replacing.as_mut()
}

/// The match asked about and its replacement.
fn current(editor: &mut Editor) -> Result<(Range<usize>, String), &'static str> {
    let state = active(editor).ok_or(NOT_REPLACING)?;
    let (_, range) = state.current.clone().ok_or("Still searching")?;
    let (query, to) = (state.query.clone(), state.to.clone());
    let to = query.replacement(editor.window_buffer().text(), range.clone(), &to);
    Ok((range, to))
}

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

pub fn plural(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        _ => format!("{count} {noun}s"),
    }
}
