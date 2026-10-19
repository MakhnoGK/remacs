use std::fs;
use std::path::{Path, PathBuf};

use remacs_core::{Candidate, CommandResult, Completions, Editor, Icon, Prompt};
use remacs_files::{
    abbreviate_home, default_directory, directory_candidates, expand_home, with_slash,
};

use crate::entry::{PARENT, SortKey};
use crate::listing::{DIRED_MODE, Listing, entry_at_point, new_listing, select_entry, set_sort};
use crate::new_file::is_creating;
use crate::wdired::is_editing;

pub const FIND_FILE: &str = "dired-find-file";
const DIRECTORY_CATEGORY: &str = "directory";
pub const NOT_A_LISTING: &str = "Not a directory listing";

/// Reads a directory name and lists it.
pub fn dired(editor: &mut Editor) -> CommandResult {
    let directory = default_directory(editor);
    let prompt = Prompt {
        label: "Dired".into(),
        category: DIRECTORY_CATEGORY,
        initial_input: with_slash(&abbreviate_home(&directory)),
        wide: true,
        hints: &[("↵", "Open"), ("⇥", "Complete path"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, input| directory_completions(input),
        move |editor, answer| {
            open_directory(editor, &directory.join(expand_home(&answer)), None, false)
        },
    )
}

/// Lists the directory of the window's file, with point on it, like Emacs `dired-jump`.
pub fn dired_jump(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    let file_name = buffer
        .path()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned());
    let directory = default_directory(editor);
    let replace = buffer.mode() == DIRED_MODE;
    let (directory, select) = match (replace, directory.parent()) {
        // Already in a listing: go up, as Emacs does.
        (true, Some(parent)) => (parent.to_path_buf(), directory_name(&directory)),
        _ => (directory, file_name),
    };
    open_directory(editor, &directory, select, replace)
}

/// Visits the file at point, or lists the directory at point in place of this listing.
pub fn find_file(editor: &mut Editor) -> CommandResult {
    if is_creating(editor) {
        return Err("Create or cancel the new file first".into());
    }
    let buffer = editor.window_buffer();
    let entry = entry_at_point(buffer).ok_or("No file on this line")?;
    let directory = buffer.default_directory().ok_or(NOT_A_LISTING)?;
    if entry.name == PARENT {
        return up_directory(editor);
    }
    let path = directory.join(&entry.name);
    if entry.is_directory {
        open_directory(editor, &path, None, true)
    } else {
        Ok(editor.visit_file(&path)?)
    }
}

pub fn up_directory(editor: &mut Editor) -> CommandResult {
    let directory = listing_directory(editor)?;
    let parent = directory.parent().ok_or("Already at the root")?;
    open_directory(editor, parent, directory_name(&directory), true)
}

/// Reads the directory again, though changes show up by themselves.
pub fn revert(editor: &mut Editor) -> CommandResult {
    let listing = editor
        .window_buffer()
        .local::<Listing>()
        .ok_or(NOT_A_LISTING)?;
    listing.reader.reread();
    Ok(())
}

pub fn sort_toggle(editor: &mut Editor) -> CommandResult {
    let current = editor
        .window_buffer()
        .local::<Listing>()
        .ok_or(NOT_A_LISTING)?
        .sort;
    sort_by(editor, current.next())
}

pub fn sort_by_name(editor: &mut Editor) -> CommandResult {
    sort_by(editor, SortKey::Name)
}

pub fn sort_by_modified(editor: &mut Editor) -> CommandResult {
    sort_by(editor, SortKey::Modified)
}

pub fn sort_by_size(editor: &mut Editor) -> CommandResult {
    sort_by(editor, SortKey::Size)
}

pub fn sort_by(editor: &mut Editor, key: SortKey) -> CommandResult {
    listing_directory(editor)?;
    if is_editing(editor) {
        return Err("Apply or cancel the edited names first".into());
    }
    set_sort(editor.window_buffer_mut(), key);
    editor.set_message(format!("Sorted by {}", key.label().to_lowercase()));
    Ok(())
}

/// Reads a name and creates that directory in the listing, with point on it once it is
/// listed, like Emacs `dired-create-directory`.
pub fn create_directory(editor: &mut Editor) -> CommandResult {
    let directory = listing_directory(editor)?;
    let prompt = Prompt {
        label: "New folder".into(),
        directory: with_slash(&abbreviate_home(&directory)),
        placeholder: "name",
        hints: &[("↵", "Create"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        move |editor, answer| {
            let answer = answer.trim();
            if answer.is_empty() {
                return Err("No name given".into());
            }
            let path = directory.join(expand_home(answer));
            if fs::symlink_metadata(&path).is_ok() {
                return Err(format!("{} already exists", abbreviate_home(&path)).into());
            }
            fs::create_dir_all(&path)?;
            let created = path
                .strip_prefix(&directory)
                .ok()
                .and_then(|relative| relative.components().next())
                .map(|first| first.as_os_str().to_string_lossy().into_owned());
            let listing = editor.buffers_mut().iter_mut().find(|buffer| {
                buffer.local::<Listing>().is_some()
                    && buffer.default_directory() == Some(directory.as_path())
            });
            if let (Some(buffer), Some(created)) = (listing, created) {
                select_entry(buffer, created);
                if let Some(listing) = buffer.local::<Listing>() {
                    listing.reader.reread();
                }
            }
            Ok(())
        },
    )
}

/// Shows a listing of `directory`, reusing one already open. With `replace`, the listing
/// in the window is killed, unless its names are being edited, so moving around keeps one
/// buffer, like Emacs `dired-kill-when-opening-new-dired-buffer`.
pub fn open_directory(
    editor: &mut Editor,
    directory: &Path,
    select: Option<String>,
    replace: bool,
) -> CommandResult {
    let directory = directory.canonicalize()?;
    if !directory.is_dir() {
        return Err(format!("Not a directory: {}", abbreviate_home(&directory)).into());
    }
    let window = editor.window_buffer();
    let left = (replace && window.local::<Listing>().is_some() && !is_editing(editor))
        .then(|| window.name().to_owned());
    let existing = editor
        .buffers()
        .iter()
        .find(|buffer| {
            buffer.local::<Listing>().is_some()
                && buffer.default_directory() == Some(directory.as_path())
        })
        .map(|buffer| buffer.name().to_owned());
    match existing {
        Some(name) => {
            editor.switch_to_buffer(&name);
            if let Some(select) = select {
                select_entry(editor.window_buffer_mut(), select);
            }
        }
        None => {
            let buffer = new_listing(directory, select, editor.notifier());
            editor.add_buffer(buffer);
        }
    }
    if let Some(left) = left
        && left != editor.window_buffer().name()
    {
        editor.kill_buffer(&left);
    }
    Ok(())
}

fn listing_directory(editor: &Editor) -> Result<PathBuf, &'static str> {
    let buffer = editor.window_buffer();
    buffer.local::<Listing>().ok_or(NOT_A_LISTING)?;
    buffer
        .default_directory()
        .map(Path::to_path_buf)
        .ok_or(NOT_A_LISTING)
}

fn directory_name(directory: &Path) -> Option<String> {
    directory
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

/// Subdirectories of the directory typed so far, led by that directory itself if it
/// exists.
pub fn directory_completions(input: &str) -> Completions {
    let mut candidates = directory_candidates(input).candidates;
    candidates.retain(|candidate| candidate.icon == Some(Icon::Directory));
    if input.ends_with('/') && expand_home(input).is_dir() {
        let itself = Candidate {
            label: "./".into(),
            icon: Some(Icon::Directory),
            ..Candidate::new(input)
        };
        candidates.insert(0, itself);
    }
    candidates.into()
}
