use remacs_core::{Buffer, CommandResult, Editor, Mark};

use crate::commands::NOT_A_LISTING;
use crate::entry::{Entry, PARENT, is_auto_save_name, is_backup_name, plural};
use crate::listing::{Listing, entry_at_point};

/// Marks the entry at point and moves to the next, like Emacs `dired-mark`.
pub fn mark(editor: &mut Editor) -> CommandResult {
    set_at_point(editor, Some(Mark::Marked))
}

/// Flags the entry at point for deletion and moves to the next, like
/// `dired-flag-file-deletion`.
pub fn flag(editor: &mut Editor) -> CommandResult {
    set_at_point(editor, Some(Mark::Flagged))
}

pub fn unmark(editor: &mut Editor) -> CommandResult {
    set_at_point(editor, None)
}

/// Moves to the previous entry and unmarks it, like `dired-unmark-backward`.
pub fn unmark_backward(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    listing_mut(buffer)?;
    let line = buffer.point_line().saturating_sub(1);
    buffer.move_to_line(line);
    set_mark(listing_mut(buffer)?, line, None);
    Ok(())
}

pub fn unmark_all(editor: &mut Editor) -> CommandResult {
    let marks = &mut listing_mut(editor.window_buffer_mut())?.marks;
    let count = marks.len();
    marks.clear();
    editor.set_message(format!("Removed {count} {}", plural(count, "mark")));
    Ok(())
}

/// Marks the unmarked entries and unmarks the marked ones; flags stay, like
/// `dired-toggle-marks`.
pub fn toggle_marks(editor: &mut Editor) -> CommandResult {
    let listing = listing_mut(editor.window_buffer_mut())?;
    for entry in listing.entries.iter().filter(|entry| entry.name != PARENT) {
        match listing.marks.get(&entry.name) {
            Some(Mark::Marked) => {
                listing.marks.remove(&entry.name);
            }
            Some(Mark::Flagged) => {}
            None => {
                listing.marks.insert(entry.name.clone(), Mark::Marked);
            }
        }
    }
    Ok(())
}

/// Flags `name~` files, like `dired-flag-backup-files`.
pub fn flag_backup_files(editor: &mut Editor) -> CommandResult {
    flag_matching(editor, is_backup_name, "backup file")
}

/// Flags `#name#` files, like `dired-flag-auto-save-files`.
pub fn flag_auto_save_files(editor: &mut Editor) -> CommandResult {
    flag_matching(editor, is_auto_save_name, "auto-save file")
}

fn flag_matching(editor: &mut Editor, matches: fn(&str) -> bool, noun: &str) -> CommandResult {
    let listing = listing_mut(editor.window_buffer_mut())?;
    let names: Vec<String> = listing
        .entries
        .iter()
        .filter(|entry| !entry.is_directory && matches(&entry.name))
        .map(|entry| entry.name.clone())
        .collect();
    let count = names.len();
    for name in names {
        listing.marks.insert(name, Mark::Flagged);
    }
    editor.set_message(format!("{count} {} flagged", plural(count, noun)));
    Ok(())
}

fn set_at_point(editor: &mut Editor, mark: Option<Mark>) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    let line = buffer.point_line();
    set_mark(listing_mut(buffer)?, line, mark);
    if line + 1 < buffer.text().line_count() {
        buffer.move_to_line(line + 1);
    }
    Ok(())
}

/// `..` stays unmarked: it is never acted on.
fn set_mark(listing: &mut Listing, line: usize, mark: Option<Mark>) {
    let Some(entry) = listing.entries.get(line).filter(|entry| entry.name != PARENT) else {
        return;
    };
    match mark {
        Some(mark) => listing.marks.insert(entry.name.clone(), mark),
        None => listing.marks.remove(&entry.name),
    };
}

fn listing_mut(buffer: &mut Buffer) -> Result<&mut Listing, &'static str> {
    buffer.local_mut::<Listing>().ok_or(NOT_A_LISTING)
}

/// The marked entries, or the one at point when none are, and whether they are marked.
pub fn marked_or_at_point(buffer: &Buffer) -> Result<(Vec<Entry>, bool), &'static str> {
    let listing = buffer.local::<Listing>().ok_or(NOT_A_LISTING)?;
    let marked: Vec<Entry> = listing
        .entries
        .iter()
        .filter(|entry| listing.marks.get(&entry.name) == Some(&Mark::Marked))
        .cloned()
        .collect();
    if !marked.is_empty() {
        return Ok((marked, true));
    }
    let at_point = entry_at_point(buffer)
        .filter(|entry| entry.name != PARENT)
        .ok_or("No file on this line")?;
    Ok((vec![at_point.clone()], false))
}
