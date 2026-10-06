use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_core::{
    Candidate, CommandResult, Completions, Editor, Icon, ListItem, Prompt, PromptOption,
};
use remacs_files::{abbreviate_home, expand_home, with_slash};

use crate::commands::{NOT_A_LISTING, directory_completions};
use crate::entry::{describe_entries, display_name, plural, size_label};
use crate::file_ops::{copy_path, move_path, remove};
use crate::jobs::{self, Report, failure_message};
use crate::listing::file_icon;
use crate::marks::marked_or_at_point;
use crate::references;

const DESTINATION_CATEGORY: &str = "dired-destination";
const RECENT_DESTINATIONS: usize = 5;
const MOVE_HINTS: &[(&str, &str)] = &[
    ("↵", "Move"),
    ("⇥", "Complete path"),
    ("M-↵", "Use path as typed"),
    ("C-g", "Cancel"),
];
const COPY_HINTS: &[(&str, &str)] = &[
    ("↵", "Copy"),
    ("⇥", "Complete path"),
    ("M-↵", "Use path as typed"),
    ("C-g", "Cancel"),
];

/// Copies the marked entries, or the one at point, like Emacs `dired-do-copy`.
pub fn copy(editor: &mut Editor) -> CommandResult {
    read_destination(editor, false)
}

/// Moves or renames the marked entries, or the one at point, like `dired-do-rename`.
pub fn rename(editor: &mut Editor) -> CommandResult {
    read_destination(editor, true)
}

/// What a copy or move does with its files, settled when the destination is read.
struct Transfer {
    directory: PathBuf,
    names: Vec<String>,
    /// E.g. `2 files`.
    what: String,
    moving: bool,
    overwrite: bool,
    /// Whether version control is told of the moves.
    record_moves: bool,
}

/// Reads where to put the files; `M-m` switches between copying and moving, `M-g` in
/// a working tree whether version control is told of a move, `M-o` whether files already
/// there are replaced.
fn read_destination(editor: &mut Editor, moving: bool) -> CommandResult {
    let buffer = editor.window_buffer();
    let directory = buffer
        .default_directory()
        .ok_or(NOT_A_LISTING)?
        .to_path_buf();
    let (sources, marked) = marked_or_at_point(buffer)?;
    let moving = Rc::new(Cell::new(moving));
    let overwrite = Rc::new(Cell::new(false));
    let record_moves = Rc::new(Cell::new(true));
    let record_option = editor
        .version_control(&directory)
        .map(|control| PromptOption {
            key: 'g',
            labels: vec![format!("Move with {}", control.move_command())],
            state: Rc::clone(&record_moves),
            shown_while: Some(Rc::clone(&moving)),
            hidden_while: None,
        });
    let mut options = vec![PromptOption {
        key: 'm',
        labels: vec!["Copy".into(), "Move".into()],
        state: Rc::clone(&moving),
        shown_while: None,
        hidden_while: None,
    }];
    options.extend(record_option);
    options.push(PromptOption::chip('o', "Overwrite existing", &overwrite));
    let bytes: u64 = sources
        .iter()
        .filter(|entry| !entry.is_directory)
        .map(|entry| entry.size)
        .sum();
    let file_count = sources.iter().filter(|entry| !entry.is_directory).count();
    let prompt = Prompt {
        label: "To".into(),
        category: DESTINATION_CATEGORY,
        initial_input: with_slash(&abbreviate_home(&directory)),
        hints: MOVE_HINTS,
        options,
        subject: describe_entries(&sources, if marked { "marked " } else { "" }),
        items: sources
            .iter()
            .map(|entry| ListItem {
                icon: Some(file_icon(entry.is_directory)),
                mark: None,
                label: display_name(entry),
                detail: entry.size_label.clone(),
                state: None,
                lines: None,
            })
            .collect(),
        items_title: "Files",
        items_summary: if file_count > 0 {
            format!("{} · {}", sources.len(), size_label(bytes))
        } else {
            sources.len().to_string()
        },
        ..Prompt::default()
    };
    let names: Vec<String> = sources.iter().map(|entry| entry.name.clone()).collect();
    let what = describe_entries(&sources, "");
    let complete = {
        let (directory, moving, count) = (directory.clone(), Rc::clone(&moving), names.len());
        move |editor: &Editor, input: &str| {
            destinations(editor, &directory, input, count, moving.get())
        }
    };
    editor.read(prompt, complete, move |editor, answer| {
        let transfer = Transfer {
            directory,
            names,
            what,
            moving: moving.get(),
            overwrite: overwrite.get(),
            record_moves: record_moves.get(),
        };
        start(editor, transfer, &answer)
    })
}

/// Directories under the input; for a path that does not exist yet, what answering with
/// it does; then the destinations used last.
fn destinations(
    editor: &Editor,
    directory: &Path,
    input: &str,
    count: usize,
    moving: bool,
) -> Completions {
    let mut completions = directory_completions(input);
    completions.hints = Some(if moving { MOVE_HINTS } else { COPY_HINTS });
    let target = directory.join(expand_home(input));
    if !input.is_empty() && fs::symlink_metadata(&target).is_err() {
        let (row, note) = new_destination(&target, input, count, moving);
        completions.candidates.insert(0, row);
        completions.note = Some(note.into());
    }
    let recent = editor
        .history(DESTINATION_CATEGORY)
        .iter()
        .filter(|path| path.as_str() != input && expand_home(path).is_dir())
        .take(RECENT_DESTINATIONS)
        .map(|path| Candidate {
            icon: Some(Icon::Directory),
            group: Some("Recent destinations".into()),
            ..Candidate::new(path.as_str())
        });
    completions.candidates.extend(recent);
    completions.total = completions.candidates.len();
    completions
}

/// A folder to create for several files or a path ending in `/`, else a new name.
fn new_destination(
    target: &Path,
    input: &str,
    count: usize,
    moving: bool,
) -> (Candidate, &'static str) {
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = target.parent().map(abbreviate_home).unwrap_or_default();
    let verb = if moving { "move" } else { "copy" };
    let keys = vec!["↵".to_owned()];
    if count > 1 || input.ends_with('/') {
        let them = if count == 1 { "it" } else { "the files" };
        let row = Candidate {
            verb: Some("Create"),
            label: format!("{name}/"),
            annotation: format!("in {parent} and {verb} {them} there"),
            icon: Some(Icon::Directory),
            keys,
            ..Candidate::new(with_slash(input))
        };
        (row, "New folder")
    } else {
        let row = Candidate {
            verb: Some(if moving { "Rename to" } else { "Copy as" }),
            label: name,
            annotation: format!("in {parent}"),
            icon: Some(Icon::File),
            keys,
            ..Candidate::new(input)
        };
        (row, "New name")
    }
}

/// Into `answer` when it is a directory, ends in `/` or several files go there, which
/// creates it if needed; else to that name.
fn start(editor: &mut Editor, transfer: Transfer, answer: &str) -> CommandResult {
    let Transfer {
        directory,
        names,
        what,
        moving,
        overwrite,
        record_moves,
    } = transfer;
    let record_moves = moving && record_moves;
    let destination = directory.join(expand_home(answer));
    let into = destination.is_dir() || answer.ends_with('/') || names.len() > 1;
    let pairs: Vec<(PathBuf, PathBuf)> = names
        .iter()
        .map(|name| {
            let to = if into {
                destination.join(name)
            } else {
                destination.clone()
            };
            (directory.join(name), to)
        })
        .collect();
    let verb = if moving { "move" } else { "copy" };
    if let Some((from, _)) = pairs.iter().find(|(from, to)| to.starts_with(from)) {
        return Err(format!("Cannot {verb} {} onto or into itself", file_name(from)).into());
    }
    if into && !destination.exists() {
        fs::create_dir_all(&destination)?;
    }
    let shown = if into {
        with_slash(&abbreviate_home(&destination))
    } else {
        abbreviate_home(&destination)
    };
    let target_directory = if into {
        destination.clone()
    } else {
        destination
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    };
    let job = Job {
        directory,
        target_directory,
        pairs,
        what,
        shown,
        moving,
        overwrite,
        record_moves,
    };
    if !moving {
        run(editor, job, Vec::new());
        return Ok(());
    }
    // Files already there are skipped, so references to them stay.
    let moves = job
        .pairs
        .iter()
        .filter(|(_, to)| overwrite || fs::symlink_metadata(to).is_err())
        .cloned()
        .collect();
    references::ask_before_moving(
        editor,
        moves,
        move |editor, edits| match references::update(editor, edits) {
            Ok(updated) => run(editor, job, updated),
            Err(error) => editor.set_message(error),
        },
    )?;
    Ok(())
}

/// A copy or move settled on.
struct Job {
    directory: PathBuf,
    target_directory: PathBuf,
    pairs: Vec<(PathBuf, PathBuf)>,
    what: String,
    /// The destination as the message names it.
    shown: String,
    moving: bool,
    overwrite: bool,
    record_moves: bool,
}

/// Copies or moves in the background; `updated` names the buffers whose references to
/// the moved files were updated.
fn run(editor: &mut Editor, job: Job, updated: Vec<String>) {
    let Job {
        directory,
        target_directory,
        pairs,
        what,
        shown,
        moving,
        overwrite,
        record_moves,
    } = job;
    let verb = if moving { "move" } else { "copy" };
    let busy = if moving { "Moving…" } else { "Copying…" };
    let count = pairs.len();
    jobs::run(editor, busy, move || {
        let (mut skipped, mut failures, mut moved) = (0, Vec::new(), Vec::new());
        for (from, to) in pairs {
            let name = file_name(&from);
            if fs::symlink_metadata(&to).is_ok() {
                if !overwrite {
                    skipped += 1;
                    continue;
                }
                if let Err(error) = remove(&to) {
                    failures.push(format!("{name}: {error}"));
                    continue;
                }
            }
            let result = if moving {
                move_path(&from, &to)
            } else {
                copy_path(&from, &to)
            };
            match result {
                Ok(()) if moving => moved.push((from, to)),
                Ok(()) => {}
                Err(error) => failures.push(format!("{name}: {error}")),
            }
        }
        let done = if moving { "Moved" } else { "Copied" };
        let mut message = failure_message(verb, &failures).unwrap_or_else(|| match skipped {
            0 => format!("{done} {what} to {shown}"),
            _ if skipped == count => {
                format!("Nothing to {verb}: already in {shown} (Overwrite existing replaces)")
            }
            _ => format!(
                "{done} to {shown}, skipping {skipped} {} already there",
                plural(skipped, "file")
            ),
        });
        if !updated.is_empty() {
            message.push_str(&format!("; updated {}", updated.join(", ")));
        }
        Report {
            directories: vec![directory, target_directory],
            moved,
            record_moves,
            message,
        }
    });
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
