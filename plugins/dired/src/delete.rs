use std::path::{Path, PathBuf};

use remacs_core::{CommandResult, Dialog, DialogOption, Editor, ListItem, Mark};
use trash::TrashContext;
use trash::macos::{DeleteMethod, TrashContextExtMacos};

use crate::commands::NOT_A_LISTING;
use crate::entry::{Entry, describe_entries, display_name};
use crate::file_ops::remove;
use crate::jobs::{self, Report, failure_message};
use crate::listing::Listing;
use crate::marks::marked_or_at_point;

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
    let (targets, shown_mark) = match mark {
        Mark::Flagged => {
            let flagged: Vec<Entry> = listing
                .entries
                .iter()
                .filter(|entry| listing.marks.get(&entry.name) == Some(&Mark::Flagged))
                .cloned()
                .collect();
            if flagged.is_empty() {
                return Err("No files flagged for deletion".into());
            }
            (flagged, Some(Mark::Flagged))
        }
        Mark::Marked => {
            let (targets, marked) = marked_or_at_point(buffer)?;
            (targets, marked.then_some(Mark::Marked))
        }
    };
    let what = describe_entries(&targets, "");
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
        title: format!("Delete {}?", describe_entries(targets, adjective)),
        explanation: explanation.into(),
        items: targets
            .iter()
            .map(|entry| ListItem {
                icon: None,
                mark,
                label: display_name(entry),
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

fn start(
    editor: &mut Editor,
    directory: PathBuf,
    names: Vec<String>,
    what: String,
    to_trash: bool,
) {
    let busy = if to_trash {
        "Moving to the Trash…"
    } else {
        "Deleting…"
    };
    jobs::run(editor, busy, move || {
        let failures = delete(&directory, &names, to_trash);
        let message = failure_message("delete", &failures).unwrap_or_else(|| {
            if to_trash {
                format!("Moved {what} to the Trash")
            } else {
                format!("Deleted {what}")
            }
        });
        Report {
            directories: vec![directory],
            moved: Vec::new(),
            message,
        }
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
