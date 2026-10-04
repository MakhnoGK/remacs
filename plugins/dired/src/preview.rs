use std::path::{Path, PathBuf};

use remacs_core::{
    Buffer, Code, Editor, FUNDAMENTAL_MODE, Notifier, Preview, PreviewBody, PreviewItem,
};
use remacs_files::abbreviate_home;

use crate::entry::{Entry, PARENT, full_modified_label, items_label, size_label};
use crate::listing::{Listing, entry_at_point, file_face, file_icon};
use crate::preview_reader::{Body, Contents, PreviewReader, PreviewRequest};

/// The preview of a listing's entry at point, read in the background.
pub struct PreviewState {
    reader: PreviewReader,
    requested: Option<PreviewRequest>,
    contents: Option<Contents>,
}

impl PreviewState {
    pub fn new(notifier: Notifier) -> Self {
        Self {
            reader: PreviewReader::start(notifier),
            requested: None,
            contents: None,
        }
    }

    fn update(&mut self, request: PreviewRequest) {
        if let Some(contents) = self.reader.latest() {
            self.contents = Some(contents);
        }
        if self.requested.as_ref() != Some(&request) {
            self.reader.request(request.clone());
            self.requested = Some(request);
        }
    }
}

/// Asks for the preview of the entry at point in the window's listing, and takes in what
/// has been read.
pub fn sync(editor: &mut Editor) {
    let buffer = editor.window_buffer();
    let Some((path, entry)) = entry_path_at_point(buffer) else {
        return;
    };
    let mode = if entry.is_directory {
        FUNDAMENTAL_MODE
    } else {
        editor.mode_for(Some(&path))
    };
    let request = PreviewRequest {
        path,
        is_directory: entry.is_directory,
        size: entry.size,
        modified: entry.modified,
        mode,
    };
    if let Some(listing) = editor.window_buffer_mut().local_mut::<Listing>() {
        listing.preview.update(request);
    }
}

/// The preview of `entry` of the listing of `directory`.
pub fn describe(listing: &Listing, directory: &Path, entry: &Entry) -> Option<Preview> {
    let path = entry_path(directory, entry)?;
    let contents = listing.preview.contents.as_ref().filter(|contents| {
        let request = &contents.request;
        request.path == path && request.size == entry.size && request.modified == entry.modified
    });
    let mode = contents.map_or(FUNDAMENTAL_MODE, |contents| contents.request.mode);
    let (kind, body) = match contents.map(|contents| &contents.body) {
        None => ("", PreviewBody::Message(String::new())),
        Some(Body::Text(text)) if text.is_empty() => (mode_label(mode), message("Empty file")),
        Some(Body::Text(text)) => {
            let mode = (mode != FUNDAMENTAL_MODE).then_some(mode);
            (
                mode_label(mode.unwrap_or(FUNDAMENTAL_MODE)),
                PreviewBody::Code(Code::new(text.clone(), mode)),
            )
        }
        Some(Body::Binary) => ("Binary", message("Binary file")),
        Some(Body::Items(items)) if items.is_empty() => ("Folder", message("Empty folder")),
        Some(Body::Items(items)) => (
            "Folder",
            PreviewBody::Items(items.iter().map(item).collect()),
        ),
        Some(Body::Failed(error)) => ("", PreviewBody::Message(error.clone())),
    };
    let size = if entry.is_directory {
        items_label(entry.size)
    } else {
        match contents.and_then(|contents| contents.line_count) {
            Some(lines) => format!("{} · {}", size_label(entry.size), lines_label(lines)),
            None => size_label(entry.size),
        }
    };
    Some(Preview {
        icon: file_icon(entry.is_directory),
        title: path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
        kind: kind.into(),
        body,
        details: vec![
            ("Path", abbreviate_home(&path)),
            ("Size", size),
            ("Modified", full_modified_label(entry.modified)),
            ("Permissions", entry.permissions.clone()),
        ],
    })
}

fn entry_path_at_point(buffer: &Buffer) -> Option<(PathBuf, &Entry)> {
    let entry = entry_at_point(buffer)?;
    Some((entry_path(buffer.default_directory()?, entry)?, entry))
}

fn entry_path(directory: &Path, entry: &Entry) -> Option<PathBuf> {
    if entry.name == PARENT {
        directory.parent().map(Path::to_path_buf)
    } else {
        Some(directory.join(&entry.name))
    }
}

fn mode_label(mode: &'static str) -> &'static str {
    if mode == FUNDAMENTAL_MODE {
        "Plain text"
    } else {
        mode
    }
}

fn message(text: &str) -> PreviewBody {
    PreviewBody::Message(text.into())
}

fn item((name, is_directory): &(String, bool)) -> PreviewItem {
    PreviewItem {
        icon: file_icon(*is_directory),
        name: name.clone(),
        face: file_face(name, *is_directory),
    }
}

fn lines_label(count: u64) -> String {
    if count == 1 {
        "1 line".into()
    } else {
        format!("{count} lines")
    }
}
