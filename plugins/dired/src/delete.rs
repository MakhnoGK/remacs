use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use remacs_core::{CommandResult, Dialog, DialogItem, DialogOption, Editor, Mark};
use trash::TrashContext;
use trash::macos::{DeleteMethod, TrashContextExtMacos};

use crate::commands::NOT_A_LISTING;
use crate::entry::{Entry, PARENT};
use crate::listing::{Listing, entry_at_point};
use crate::marks::plural;

/// Deletions running in the background, reported once done.
#[derive(Default)]
pub struct Deletions {
    reports: Vec<Receiver<Report>>,
}

struct Report {
    directory: PathBuf,
    /// What was deleted, e.g. `2 files`.
    what: String,
    to_trash: bool,
    /// `name: error`
    failures: Vec<String>,
}

/// Deletes the flagged entries after asking, like Emacs `dired-do-flagged-delete`.
pub fn delete_flagged(editor: &mut Editor) -> CommandResult {
    confirm(editor, Mark::Flagged)
}

/// Deletes the marked entries, or the one at point, after asking, like `dired-do-delete`.
pub fn delete_marked(editor: &mut Editor) -> CommandResult {
    confirm(editor, Mark::Marked)
}

fn confirm(editor: &mut Editor, mark: Mark) -> CommandResult {
    let buffer = editor.window_buffer();
    let listing = buffer.local::<Listing>().ok_or(NOT_A_LISTING)?;
    let directory = buffer
        .default_directory()
        .ok_or(NOT_A_LISTING)?
        .to_path_buf();
    let mut targets: Vec<Entry> = listing
        .entries
        .iter()
        .filter(|entry| listing.marks.get(&entry.name) == Some(&mark))
        .cloned()
        .collect();
    let mut shown_mark = Some(mark);
    if targets.is_empty() && mark == Mark::Marked {
        targets.extend(
            entry_at_point(buffer)
                .filter(|entry| entry.name != PARENT)
                .cloned(),
        );
        shown_mark = None;
    }
    if targets.is_empty() {
        return Err(match mark {
            Mark::Flagged => "No files flagged for deletion",
            Mark::Marked => "No file on this line",
        }
        .into());
    }
    let what = describe_targets(&targets, "");
    let names: Vec<String> = targets.iter().map(|entry| entry.name.clone()).collect();
    editor.ask_dialog(
        true,
        move |to_trash| dialog(&targets, shown_mark, to_trash),
        move |editor, yes, to_trash| {
            if yes {
                start(editor, directory, names, what, to_trash);
            }
            Ok(())
        },
    )
}

fn dialog(targets: &[Entry], mark: Option<Mark>, to_trash: bool) -> Dialog {
    let adjective = match mark {
        Some(Mark::Flagged) => "flagged ",
        Some(Mark::Marked) => "marked ",
        None => "",
    };
    let single = targets.len() == 1;
    let with_folders = targets.iter().any(|entry| entry.is_directory);
    let explanation = match (to_trash, single, with_folders) {
        (true, true, _) => "It goes to the Trash, so you can still restore it.",
        (true, false, _) => "They go to the Trash, so you can still restore them.",
        (false, true, false) => "It is deleted for good.",
        (false, true, true) => "It is deleted for good, with everything in it.",
        (false, false, false) => "They are deleted for good.",
        (false, false, true) => "They are deleted for good, folders with everything in them.",
    };
    Dialog {
        title: format!("Delete {}?", describe_targets(targets, adjective)),
        explanation: explanation.into(),
        items: targets
            .iter()
            .map(|entry| DialogItem {
                mark,
                label: if entry.is_directory {
                    format!("{}/", entry.name)
                } else {
                    entry.name.clone()
                },
                detail: entry.size_label.clone(),
            })
            .collect(),
        option: Some(DialogOption {
            key: 't',
            label: "Move to Trash",
            on: to_trash,
        }),
        yes_label: if to_trash { "Move to Trash" } else { "Delete" }.into(),
        no_label: "Cancel",
        destructive: true,
    }
}

/// `buffer.rs`, `2 files`, `3 flagged folders` or `4 marked items`.
fn describe_targets(targets: &[Entry], adjective: &str) -> String {
    if let [single] = targets {
        return single.name.clone();
    }
    let directories = targets.iter().filter(|entry| entry.is_directory).count();
    let noun = match directories {
        0 => "file",
        count if count == targets.len() => "folder",
        _ => "item",
    };
    format!(
        "{} {adjective}{}",
        targets.len(),
        plural(targets.len(), noun)
    )
}

fn start(
    editor: &mut Editor,
    directory: PathBuf,
    names: Vec<String>,
    what: String,
    to_trash: bool,
) {
    let (send, receive) = mpsc::channel();
    let notifier = editor.notifier();
    thread::Builder::new()
        .name("dired-delete".into())
        .spawn(move || {
            let failures = delete(&directory, &names, to_trash);
            let _ = send.send(Report {
                directory,
                what,
                to_trash,
                failures,
            });
            notifier();
        })
        .expect("spawning the dired delete thread");
    editor.plugin_state::<Deletions>().reports.push(receive);
    editor.set_message(if to_trash {
        "Moving to the Trash…"
    } else {
        "Deleting…"
    });
}

fn delete(directory: &Path, names: &[String], to_trash: bool) -> Vec<String> {
    let mut trash = TrashContext::default();
    // Finder's way asks for permission to script it, and plays a sound.
    trash.set_delete_method(DeleteMethod::NsFileManager);
    names
        .iter()
        .filter_map(|name| {
            let path = directory.join(name);
            let deleted = if to_trash {
                trash.delete(&path).map_err(|error| error.to_string())
            } else {
                remove(&path).map_err(|error| error.to_string())
            };
            deleted.err().map(|error| format!("{name}: {error}"))
        })
        .collect()
}

/// A directory with everything in it; a link, not what it points to.
fn remove(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Says how finished deletions went, and reads their directories again.
pub fn pick_up_deletions(editor: &mut Editor) {
    let mut reports = Vec::new();
    editor
        .plugin_state::<Deletions>()
        .reports
        .retain(|receiver| match receiver.try_recv() {
            Ok(report) => {
                reports.push(report);
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
    for report in reports {
        for buffer in editor.buffers() {
            if buffer.default_directory() == Some(report.directory.as_path())
                && let Some(listing) = buffer.local::<Listing>()
            {
                listing.reader.reread();
            }
        }
        editor.set_message(match report.failures.as_slice() {
            [] if report.to_trash => format!("Moved {} to the Trash", report.what),
            [] => format!("Deleted {}", report.what),
            [failure] => format!("Could not delete {failure}"),
            [failure, rest @ ..] => {
                format!("Could not delete {failure} (and {} more)", rest.len())
            }
        });
    }
}
