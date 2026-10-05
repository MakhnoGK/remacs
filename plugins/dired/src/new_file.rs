//! Creating an empty file in place: a row opens under point with a field for its name,
//! like the names wdired edits.

use std::fs::File;
use std::io;

use remacs_core::{Buffer, CommandResult, Editor};

use crate::commands::NOT_A_LISTING;
use crate::entry::Entry;
use crate::listing::{Listing, line_text, redraw, select_entry};
use crate::wdired::invalid_name;

pub const HINTS: &[(&str, &str)] = &[("↵", "Create"), ("C-g", "Cancel")];
const NOT_CREATING: &str = "Not creating a file";

pub fn is_creating(editor: &Editor) -> bool {
    editor
        .buffer()
        .local::<Listing>()
        .is_some_and(|listing| listing.new_file.is_some())
}

/// Opens an empty row under point for the new file's name.
pub fn start(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    let point_line = buffer.point_line();
    let listing = buffer.local_mut::<Listing>().ok_or(NOT_A_LISTING)?;
    if !listing.is_loaded() {
        return Err("Still reading the directory".into());
    }
    let line = (point_line + 1).min(listing.entries.len());
    listing.entries.insert(line, Entry::default());
    redraw(buffer);
    if let Some(listing) = buffer.local_mut::<Listing>() {
        listing.new_file = Some(line);
    }
    buffer.set_read_only(false);
    let start = buffer.text().line_start(line).unwrap_or_default();
    buffer.set_point(start);
    Ok(())
}

/// Creates the file named in the row, with point on it once it is listed; a name already
/// listed is refused and the row stays for another.
pub fn finish(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    let line = new_file_line(buffer.local::<Listing>())?;
    let name = line_text(buffer, line).trim().to_owned();
    if name.is_empty() {
        return cancel(editor);
    }
    if let Some(reason) = invalid_name(&name) {
        return Err(format!("Cannot create {name}: the name {reason}").into());
    }
    let listing = buffer.local::<Listing>().ok_or(NOT_A_LISTING)?;
    // A case-insensitive volume, as macOS's are by default, sees these as one name.
    if let Some(existing) = listing
        .entries
        .iter()
        .find(|entry| entry.name.to_lowercase() == name.to_lowercase())
    {
        return Err(format!("{} already exists", existing.name).into());
    }
    let path = buffer.default_directory().ok_or(NOT_A_LISTING)?.join(&name);
    match File::create_new(&path) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Err(format!("{name} already exists").into());
        }
        Err(error) => return Err(format!("Could not create {name}: {error}").into()),
        Ok(_) => {}
    }
    stop(editor)?;
    let buffer = editor.window_buffer_mut();
    select_entry(buffer, name);
    if let Some(listing) = buffer.local::<Listing>() {
        listing.reader.reread();
    }
    Ok(())
}

/// Removes the row, creating nothing.
pub fn cancel(editor: &mut Editor) -> CommandResult {
    stop(editor)?;
    // What changed on disk meanwhile was not listed.
    if let Some(listing) = editor.window_buffer().local::<Listing>() {
        listing.reader.reread();
    }
    Ok(())
}

/// Keeps the name on its one line, with point in it. Runs after every command.
pub fn after_command(editor: &mut Editor) {
    let buffer = editor.window_buffer_mut();
    let Ok(line) = new_file_line(buffer.local::<Listing>()) else {
        return;
    };
    if !lines_kept(buffer) {
        if !buffer.undo() || !lines_kept(buffer) {
            // Nothing left to undo it with: the row empty again.
            let new_file = buffer
                .local_mut::<Listing>()
                .and_then(|listing| listing.new_file.take());
            redraw(buffer);
            if let Some(listing) = buffer.local_mut::<Listing>() {
                listing.new_file = new_file;
            }
        }
        editor.set_message("The name stays on one line");
    }
    let buffer = editor.window_buffer_mut();
    let range = buffer.text().line_range(line).unwrap_or_default();
    let point = buffer.point().clamp(range.start, range.end);
    if point != buffer.point() {
        buffer.set_point(point);
    }
}

/// The row taken out, the listing read-only again with point back where it was.
fn stop(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    let listing = buffer.local_mut::<Listing>().ok_or(NOT_A_LISTING)?;
    let line = listing.new_file.take().ok_or(NOT_CREATING)?;
    listing.entries.remove(line);
    buffer.set_read_only(true);
    redraw(buffer);
    let start = buffer
        .text()
        .line_start(line.saturating_sub(1))
        .unwrap_or_default();
    buffer.set_point(start);
    Ok(())
}

fn new_file_line(listing: Option<&Listing>) -> Result<usize, &'static str> {
    listing.ok_or(NOT_A_LISTING)?.new_file.ok_or(NOT_CREATING)
}

fn lines_kept(buffer: &Buffer) -> bool {
    buffer
        .local::<Listing>()
        .is_some_and(|listing| buffer.text().line_count() == listing.entries.len())
}
