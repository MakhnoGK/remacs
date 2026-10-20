use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_core::{
    Buffer, Button, CellStyle, Column, Editor, Field, FileState, Icon, Mark, Notice, Notifier, Row,
    RowFace, StatusItem, StatusSegment, Table, Tone,
};
use remacs_files::abbreviate_home;
use remacs_text::Text;

use crate::commands::{FIND_FILE, open_directory, sort_by};
use crate::entry::{Entry, PARENT, SortKey, display_name, is_dim_name, items_label, sort};
use crate::preview::{self, PreviewState};
use crate::reader::DirectoryReader;
use crate::wdired::{self, Editing};

pub const DIRED_MODE: &str = "Dired";

const COLUMNS: &[Column] = &[
    Column {
        title: "Name",
        width: None,
        style: CellStyle::Primary,
    },
    Column {
        title: "",
        width: Some(14.0),
        style: CellStyle::State,
    },
    Column {
        title: "Size",
        width: Some(72.0),
        style: CellStyle::Figure,
    },
    Column {
        title: "Modified",
        width: Some(104.0),
        style: CellStyle::Label,
    },
    Column {
        title: "Permissions",
        width: Some(92.0),
        style: CellStyle::Faint,
    },
];

const HINTS: &[(&str, &str)] = &[
    ("↵", "Open"),
    ("m", "Mark"),
    ("d", "Flag"),
    ("x", "Delete flagged"),
    ("C", "Copy"),
    ("R", "Move"),
    ("c", "File"),
    ("+", "Folder"),
];

/// A directory listing's state; its buffer's lines are the entries' names, in order.
pub struct Listing {
    pub entries: Vec<Entry>,
    pub sort: SortKey,
    loaded: bool,
    /// Entry to put point on once the listing is read, e.g. the directory just left.
    select: Option<String>,
    pub reader: DirectoryReader,
    pub preview: PreviewState,
    /// By entry name, so they survive sorting and reading again.
    pub marks: HashMap<String, Mark>,
    /// While the names are edited, the lines stand for the entries as they are, so the
    /// listing is neither read again nor sorted.
    pub editing: Option<Editing>,
}

impl Listing {
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Whether the lines are being edited, so they must stay as they are.
    fn is_frozen(&self) -> bool {
        self.editing.is_some()
    }
}

/// A read-only buffer listing `directory`, read in the background.
pub fn new_listing(directory: PathBuf, select: Option<String>, notifier: Notifier) -> Buffer {
    let name = directory.file_name().map_or_else(
        || directory.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let preview = PreviewState::new(notifier.clone());
    let reader = DirectoryReader::start(directory.clone(), notifier);
    let mut buffer = Buffer::new(name, Text::default(), DIRED_MODE)
        .with_directory(directory)
        .read_only();
    buffer.set_local(Listing {
        entries: Vec::new(),
        sort: SortKey::default(),
        loaded: false,
        select,
        reader,
        preview,
        marks: HashMap::new(),
        editing: None,
    });
    buffer
}

pub fn entry_at_point(buffer: &Buffer) -> Option<&Entry> {
    let listing = buffer.local::<Listing>()?;
    listing.entries.get(buffer.point_line())
}

/// Takes in what the background readers have read since the last call.
pub fn pick_up_listings(editor: &mut Editor) {
    let mut failure = None;
    for buffer in editor.buffers_mut() {
        let Some(listing) = buffer
            .local::<Listing>()
            .and_then(|listing| listing.reader.latest())
        else {
            continue;
        };
        match listing {
            Ok(entries) => set_entries(buffer, entries),
            Err(error) => failure = Some(format!("{}: {error}", buffer.name())),
        }
    }
    if let Some(failure) = failure {
        editor.set_error(failure);
    }
}

/// Shows `entries`, keeping point on the entry it was on.
pub fn set_entries(buffer: &mut Buffer, mut entries: Vec<Entry>) {
    let current = entry_at_point(buffer).map(|entry| entry.name.clone());
    let Some(listing) = buffer
        .local_mut::<Listing>()
        .filter(|listing| !listing.is_frozen())
    else {
        return;
    };
    sort(&mut entries, listing.sort);
    // Marks of files that are gone go with them.
    let names: HashSet<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    listing
        .marks
        .retain(|name, _| names.contains(name.as_str()));
    listing.entries = entries;
    let target = listing.select.take().or(current);
    let first_load = !std::mem::replace(&mut listing.loaded, true);
    redisplay(buffer, target, first_load);
}

pub fn set_sort(buffer: &mut Buffer, key: SortKey) {
    let current = entry_at_point(buffer).map(|entry| entry.name.clone());
    let Some(listing) = buffer
        .local_mut::<Listing>()
        .filter(|listing| !listing.is_frozen())
    else {
        return;
    };
    listing.sort = key;
    sort(&mut listing.entries, key);
    redisplay(buffer, current, false);
}

/// Puts point on the entry called `name`, now or once a read lists it.
pub fn select_entry(buffer: &mut Buffer, name: String) {
    let Some(listing) = buffer.local_mut::<Listing>() else {
        return;
    };
    if listing.entries.iter().any(|entry| entry.name == name) {
        redisplay(buffer, Some(name), false);
    } else {
        listing.select = Some(name);
    }
}

/// Rewrites the buffer from the entries, point staying on its line.
pub fn redraw(buffer: &mut Buffer) {
    redisplay(buffer, None, false);
}

/// Rewrites the buffer from the entries, unless their lines are being edited; point goes
/// to `target`, else stays on its line, or on the first real entry the first time.
fn redisplay(buffer: &mut Buffer, target: Option<String>, first_load: bool) {
    let Some(listing) = buffer
        .local::<Listing>()
        .filter(|listing| !listing.is_frozen())
    else {
        return;
    };
    let names: Vec<String> = listing
        .entries
        .iter()
        .map(|entry| entry.name.replace('\n', "?"))
        .collect();
    let has_parent = listing
        .entries
        .first()
        .is_some_and(|entry| entry.name == PARENT);
    let target_line =
        target.and_then(|name| listing.entries.iter().position(|entry| entry.name == name));
    let line = target_line.unwrap_or(if first_load {
        usize::from(has_parent && names.len() > 1)
    } else {
        buffer.point_line()
    });
    buffer.replace_text(Text::from(names.join("\n").as_str()));
    let last_line = buffer.text().line_count().saturating_sub(1);
    let offset = buffer
        .text()
        .line_start(line.min(last_line))
        .unwrap_or_default();
    buffer.set_point(offset);
}

pub fn table(editor: &Editor, lines: Range<usize>) -> Option<Table> {
    let buffer = editor.window_buffer();
    let listing = buffer.local::<Listing>()?;
    let directory = buffer.default_directory()?;
    let shown = lines.start.min(listing.entries.len())..lines.end.min(listing.entries.len());
    let editing = listing.editing.as_ref();
    let rows = listing.entries[shown.clone()]
        .iter()
        .zip(shown)
        .map(|(entry, line)| {
            let edited = editing
                .filter(|_| entry.name != PARENT)
                .map(|_| line_text(buffer, line));
            let state = (entry.name != PARENT)
                .then(|| editor.file_status(&directory.join(&entry.name)))
                .flatten()
                .map(|status| status.state);
            row(
                entry,
                listing.marks.get(&entry.name).copied(),
                edited,
                state,
            )
        })
        .collect();
    let frozen = listing.is_frozen();
    let sort = if frozen {
        Vec::new()
    } else {
        SortKey::ALL
            .into_iter()
            .map(|key| Button {
                label: key.label().into(),
                selected: key == listing.sort,
                action: Rc::new(move |editor| sort_by(editor, key)),
            })
            .collect()
    };
    let preview = (!frozen)
        .then(|| {
            entry_at_point(buffer)
                .and_then(|entry| preview::describe(editor, listing, directory, entry))
        })
        .flatten();
    let hints = if editing.is_some() {
        wdired::HINTS
    } else {
        HINTS
    };
    Some(Table {
        crumbs: crumbs(directory),
        sort,
        columns: COLUMNS,
        rows,
        activate: FIND_FILE,
        hints,
        preview,
        notice: editing.map(|_| Notice {
            label: "Editing names",
            keys: wdired::NOTICE_KEYS,
        }),
        changes: editing.map(|editing| wdired::changes(editor, editing)),
    })
}

fn line_text(buffer: &Buffer, line: usize) -> String {
    let range = buffer.text().line_range(line).unwrap_or_default();
    buffer.contents(range)
}

pub fn status(editor: &Editor) -> Option<StatusSegment> {
    let listing = editor.window_buffer().local::<Listing>()?;
    let label = if listing.loaded {
        let has_parent = listing
            .entries
            .first()
            .is_some_and(|entry| entry.name == PARENT);
        let listed = listing.entries.len() - usize::from(has_parent);
        items_label(listed as u64)
    } else {
        "Reading…".into()
    };
    Some(StatusSegment {
        items: vec![StatusItem::new(None, label)],
        command: None,
        active: false,
        code: false,
    })
}

/// How many entries are marked and flagged, when any are.
pub fn marks_status(editor: &Editor) -> Option<StatusSegment> {
    let listing = editor.window_buffer().local::<Listing>()?;
    let count = |mark| listing.marks.values().filter(|&&each| each == mark).count();
    let (marked, flagged) = (count(Mark::Marked), count(Mark::Flagged));
    let items: Vec<_> = [
        (marked, "marked", Tone::Accent),
        (flagged, "flagged", Tone::Danger),
    ]
    .into_iter()
    .filter(|&(count, ..)| count > 0)
    .map(|(count, label, tone)| StatusItem::toned(format!("{count} {label}"), tone))
    .collect();
    (!items.is_empty()).then_some(StatusSegment {
        items,
        command: None,
        active: false,
        code: false,
    })
}

/// With `edited`, the name as it is being edited, in a field.
fn row(entry: &Entry, mark: Option<Mark>, edited: Option<String>, state: Option<FileState>) -> Row {
    let field = edited.as_ref().map(|name| Field {
        original: (*name != entry.name).then(|| entry.name.clone()),
    });
    Row {
        mark,
        icon: Some(file_icon(entry.is_directory)),
        cells: vec![
            edited.unwrap_or_else(|| display_name(entry)),
            state.map(FileState::letter).unwrap_or_default().to_owned(),
            entry.size_label.clone(),
            entry.modified_label.clone(),
            entry.permissions.clone(),
        ],
        face: file_face(&entry.name, entry.is_directory),
        field,
        state,
    }
}

pub fn file_icon(is_directory: bool) -> Icon {
    if is_directory {
        Icon::Directory
    } else {
        Icon::File
    }
}

pub fn file_face(name: &str, is_directory: bool) -> RowFace {
    if is_dim_name(name) {
        RowFace::Dim
    } else if is_directory {
        RowFace::Directory
    } else {
        RowFace::Normal
    }
}

/// The directories down to `directory`, from `~` when it is under home; each opens its
/// listing with point on the way back down.
fn crumbs(directory: &Path) -> Vec<Button> {
    let mut ancestors: Vec<&Path> = directory.ancestors().collect();
    ancestors.reverse();
    let home = ancestors
        .iter()
        .position(|ancestor| abbreviate_home(ancestor) == "~");
    let shown = &ancestors[home.unwrap_or(0)..];
    shown
        .iter()
        .enumerate()
        .map(|(index, &path)| {
            let label = match path.file_name() {
                _ if home.is_some() && index == 0 => "~".to_owned(),
                Some(name) => name.to_string_lossy().into_owned(),
                None => "/".to_owned(),
            };
            let target = path.to_path_buf();
            let child = shown
                .get(index + 1)
                .and_then(|child| child.file_name())
                .map(|name| name.to_string_lossy().into_owned());
            Button {
                label,
                selected: index + 1 == shown.len(),
                action: Rc::new(move |editor| open_directory(editor, &target, child.clone(), true)),
            }
        })
        .collect()
}
