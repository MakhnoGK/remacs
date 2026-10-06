use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{
    Changes, DiffLine, Editor, Elsewhere, FileDiff, FileEdits, Mark, Preview, PreviewBody, RowFace,
    Tone, parse_sequence,
};
use remacs_editing::EditingPlugin;
use remacs_files::FilesPlugin;
use remacs_minibuffer::MinibufferPlugin;

use crate::commands::open_directory;
use crate::entry::{
    Entry, LocalTime, PARENT, SortKey, date_label, full_date_label, items_label, mode_label,
    size_label, sort,
};
use crate::listing::{DIRED_MODE, Listing};
use crate::{DiredPlugin, deliver_rename_edits, take_rename_requests};

const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// A throwaway directory: a subdirectory, a file, a hidden file and a backup.
struct Tree(PathBuf);

impl Tree {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("remacs-dired-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub/deeper")).unwrap();
        fs::write(root.join("sub/inner.rs"), "").unwrap();
        fs::write(root.join("lib.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join(".hidden"), "").unwrap();
        fs::write(root.join("lib.rs~"), "").unwrap();
        Self(root.canonicalize().unwrap())
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn editor() -> Editor {
    let mut editor = Editor::new(&[
        &EditingPlugin,
        &MinibufferPlugin,
        &FilesPlugin,
        &DiredPlugin,
    ]);
    editor.viewport.set_height(20);
    editor
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn lines(editor: &Editor) -> Vec<String> {
    let buffer = editor.window_buffer();
    buffer
        .contents(0..buffer.text().len())
        .split('\n')
        .map(str::to_owned)
        .collect()
}

fn line_at_point(editor: &Editor) -> String {
    lines(editor)[editor.window_buffer().point_line()].clone()
}

/// Refreshes until the window's listing satisfies `done`, as background reads arrive.
fn wait_until(editor: &mut Editor, done: impl Fn(&Editor) -> bool) {
    let start = Instant::now();
    while !done(editor) {
        assert!(start.elapsed() < READ_TIMEOUT, "listing never arrived");
        thread::sleep(Duration::from_millis(10));
        editor.refresh();
    }
}

fn wait_for_listing(editor: &mut Editor) {
    wait_until(editor, |editor| {
        editor
            .window_buffer()
            .local::<Listing>()
            .is_some_and(|listing| !listing.entries.is_empty())
    });
}

fn entry(name: &str, is_directory: bool, size: u64, modified_secs: u64) -> Entry {
    Entry {
        name: name.into(),
        is_directory,
        size,
        modified: Some(std::time::UNIX_EPOCH + Duration::from_secs(modified_secs)),
        size_label: String::new(),
        modified_label: String::new(),
        permissions: String::new(),
    }
}

#[test]
fn sizes_dates_and_permissions_read_like_finder_and_ls() {
    assert_eq!(size_label(512), "512 B");
    assert_eq!(size_label(5_600), "5.6 KB");
    assert_eq!(size_label(182_000), "182 KB");
    assert_eq!(size_label(999_999), "1.0 MB");
    assert_eq!(items_label(1), "1 item");
    assert_eq!(items_label(3), "3 items");
    let time = LocalTime {
        year: 2026,
        month: 9,
        day: 4,
        hour: 9,
        minute: 5,
    };
    assert_eq!(date_label(time, 2026), "Oct 4, 09:05");
    assert_eq!(date_label(time, 2027), "Oct 4, 2026");
    let day = |day| LocalTime { day, ..time };
    assert_eq!(full_date_label(time, time, day(3)), "Today, 09:05");
    assert_eq!(full_date_label(time, day(5), time), "Yesterday, 09:05");
    assert_eq!(full_date_label(time, day(9), day(8)), "Oct 4, 09:05");
    let next_year = LocalTime { year: 2027, ..time };
    assert_eq!(
        full_date_label(time, next_year, next_year),
        "Oct 4, 2026, 09:05"
    );
    assert_eq!(mode_label(0o755), "rwxr-xr-x");
    assert_eq!(mode_label(0o4755), "rwsr-xr-x");
    assert_eq!(mode_label(0o1777), "rwxrwxrwt");
    assert_eq!(mode_label(0o2644), "rw-r-Sr--");
}

#[test]
fn directories_come_first_whatever_the_order() {
    let mut entries = vec![
        entry("small.rs", false, 10, 300),
        entry("Big.rs", false, 900, 100),
        entry("zeta", true, 2, 50),
        entry(PARENT, true, 0, 0),
        entry("alpha", true, 7, 400),
    ];
    let names = |entries: &[Entry]| -> Vec<String> {
        entries.iter().map(|entry| entry.name.clone()).collect()
    };
    sort(&mut entries, SortKey::Name);
    assert_eq!(
        names(&entries),
        ["..", "alpha", "zeta", "Big.rs", "small.rs"]
    );
    sort(&mut entries, SortKey::Modified);
    assert_eq!(
        names(&entries),
        ["..", "alpha", "zeta", "small.rs", "Big.rs"]
    );
    sort(&mut entries, SortKey::Size);
    assert_eq!(
        names(&entries),
        ["..", "alpha", "zeta", "Big.rs", "small.rs"]
    );
}

#[test]
fn a_listing_is_a_read_only_table_with_point_on_the_first_entry() {
    let tree = Tree::new("listing");
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);

    assert_eq!(editor.window_buffer().mode(), DIRED_MODE);
    assert_eq!(
        lines(&editor),
        ["..", "sub", ".hidden", "lib.rs", "lib.rs~"]
    );
    assert_eq!(line_at_point(&editor), "sub");

    let table = editor.table(0..20).unwrap();
    assert_eq!(table.rows.len(), 5);
    assert_eq!(table.rows[1].cells[..3], ["sub/", "", "2 items"]);
    assert_eq!(table.rows[1].face, RowFace::Directory);
    assert_eq!(table.rows[2].face, RowFace::Dim);
    assert_eq!(table.rows[3].cells[2], "13 B");
    assert_eq!(table.rows[3].cells[4], "-rw-r--r--");
    assert!(table.rows.iter().all(|row| row.state.is_none()));
    assert_eq!(
        table.crumbs.last().unwrap().label,
        editor.window_buffer().name()
    );
    let status = editor.status_segments();
    assert_eq!(status[0].items[0].text, "4 items");

    press(&mut editor, "z");
    assert!(editor.echo().unwrap().starts_with("Buffer is read-only"));
    assert_eq!(lines(&editor).len(), 5);
}

#[test]
fn opening_a_directory_replaces_the_listing_and_going_up_returns_to_it() {
    let tree = Tree::new("navigate");
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);
    let top = editor.window_buffer().name().to_owned();

    press(&mut editor, "RET");
    wait_for_listing(&mut editor);
    assert_eq!(editor.window_buffer().name(), "sub");
    assert_eq!(lines(&editor), ["..", "deeper", "inner.rs"]);
    let listings = |editor: &Editor| {
        editor
            .buffers()
            .iter()
            .filter(|buffer| buffer.mode() == DIRED_MODE)
            .count()
    };
    assert_eq!(listings(&editor), 1);

    press(&mut editor, "^");
    wait_for_listing(&mut editor);
    assert_eq!(editor.window_buffer().name(), top);
    assert_eq!(line_at_point(&editor), "sub");
    assert_eq!(listings(&editor), 1);

    press(&mut editor, "n n RET");
    assert_eq!(editor.window_buffer().name(), "lib.rs");
    press(&mut editor, "C-x C-j");
    wait_for_listing(&mut editor);
    assert_eq!(line_at_point(&editor), "lib.rs");
}

#[test]
fn sorting_keeps_point_on_its_entry() {
    let tree = Tree::new("sort");
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);
    press(&mut editor, "n n");
    assert_eq!(line_at_point(&editor), "lib.rs");

    press(&mut editor, "s s");
    let listing = editor.window_buffer().local::<Listing>().unwrap();
    assert_eq!(listing.sort, SortKey::Size);
    assert_eq!(lines(&editor)[2], "lib.rs");
    assert_eq!(line_at_point(&editor), "lib.rs");
}

#[test]
fn changes_on_disk_show_up_by_themselves() {
    let tree = Tree::new("watch");
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);

    fs::write(tree.0.join("added.txt"), "").unwrap();
    wait_until(&mut editor, |editor| {
        lines(editor).contains(&"added.txt".to_owned())
    });
    assert_eq!(line_at_point(&editor), "sub");
}

fn preview(editor: &Editor) -> Option<Preview> {
    editor.table(0..20)?.preview
}

#[test]
fn the_preview_shows_the_items_of_a_directory_and_the_head_of_a_file() {
    let tree = Tree::new("preview");
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);

    wait_until(&mut editor, |editor| {
        preview(editor).is_some_and(|preview| matches!(preview.body, PreviewBody::Items(_)))
    });
    let directory = preview(&editor).unwrap();
    assert_eq!(
        (directory.title.as_str(), directory.kind.as_str()),
        ("sub", "Folder")
    );
    let PreviewBody::Items(items) = directory.body else {
        unreachable!()
    };
    let names: Vec<_> = items
        .iter()
        .map(|item| (item.name.as_str(), item.face))
        .collect();
    assert_eq!(
        names,
        [
            ("deeper", RowFace::Directory),
            ("inner.rs", RowFace::Normal)
        ]
    );
    assert_eq!(directory.details[1], ("Size", "2 items".to_owned()));

    press(&mut editor, "n n");
    wait_until(&mut editor, |editor| {
        preview(editor).is_some_and(|preview| preview.details[1].1.contains("line"))
    });
    let file = preview(&editor).unwrap();
    assert_eq!(
        (file.title.as_str(), file.kind.as_str()),
        ("lib.rs", "Plain text")
    );
    let PreviewBody::Code(code) = file.body else {
        unreachable!()
    };
    assert_eq!(code.text, "fn main() {}");
    assert_eq!(file.details[1], ("Size", "13 B · 1 line".to_owned()));
    assert_eq!(file.details[3], ("Permissions", "-rw-r--r--".to_owned()));
}

fn marks(editor: &Editor) -> Vec<Option<Mark>> {
    let table = editor.table(0..20).unwrap();
    table.rows.iter().map(|row| row.mark).collect()
}

#[test]
fn marks_and_flags_move_point_on_and_are_counted() {
    let tree = Tree::new("marks");
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);
    let (marked, flagged) = (Some(Mark::Marked), Some(Mark::Flagged));

    press(&mut editor, "m d");
    assert_eq!(line_at_point(&editor), "lib.rs");
    assert_eq!(marks(&editor), [None, marked, flagged, None, None]);
    let counts = &editor.status_segments()[0].items;
    assert_eq!(counts[0].text, "1 marked");
    assert_eq!(counts[0].tone, Tone::Accent);
    assert_eq!(counts[1].text, "1 flagged");

    press(&mut editor, "DEL");
    assert_eq!(line_at_point(&editor), ".hidden");
    press(&mut editor, "t ~");
    assert_eq!(marks(&editor), [None, None, marked, marked, flagged]);

    press(&mut editor, "M-< m");
    assert_eq!(marks(&editor)[0], None, "`..` is never marked");
    press(&mut editor, "U");
    assert!(marks(&editor).iter().all(Option::is_none));
    assert_eq!(editor.status_segments()[0].items[0].text, "4 items");
}

#[test]
fn deleting_flagged_files_asks_first_and_keeps_marks_of_the_rest() {
    let tree = Tree::new("delete");
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);

    press(&mut editor, "x");
    assert_eq!(editor.echo().unwrap(), "No files flagged for deletion");
    press(&mut editor, "~ n m");
    assert_eq!(line_at_point(&editor), "lib.rs");
    press(&mut editor, "x");
    let dialog = editor.minibuffer().unwrap().dialog().unwrap();
    assert_eq!(dialog.title, "Delete lib.rs~?");
    assert_eq!(dialog.yes_label, "Move to Trash");
    assert_eq!(dialog.items[0].mark, Some(Mark::Flagged));

    // Deleting for good keeps the user's Trash out of the test.
    press(&mut editor, "t");
    let dialog = editor.minibuffer().unwrap().dialog().unwrap();
    assert_eq!(dialog.yes_label, "Delete");
    assert!(!dialog.option.unwrap().on);
    press(&mut editor, "y");
    wait_until(&mut editor, |editor| {
        !lines(editor).contains(&"lib.rs~".to_owned())
    });
    assert!(!tree.0.join("lib.rs~").exists());
    assert_eq!(editor.echo().unwrap(), "Deleted lib.rs~");
    assert_eq!(marks(&editor)[2], Some(Mark::Marked));

    press(&mut editor, "U M-< n n n D");
    assert_eq!(line_at_point(&editor), "lib.rs");
    assert_eq!(
        editor.minibuffer().unwrap().dialog().unwrap().title,
        "Delete lib.rs?"
    );
    press(&mut editor, "n");
    assert!(editor.minibuffer().is_none());
    assert!(tree.0.join("lib.rs").exists());
}

#[test]
fn a_new_folder_is_listed_with_point_on_it() {
    let tree = Tree::new("mkdir");
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);

    press(&mut editor, "+ n e w RET");
    assert!(tree.0.join("new").is_dir());
    wait_until(&mut editor, |editor| line_at_point(editor) == "new");

    press(&mut editor, "+ n e w RET");
    assert!(editor.echo().unwrap().ends_with("new already exists"));
}

#[test]
fn a_new_file_is_named_in_a_row_under_point_and_listed_with_point_on_it() {
    let tree = Tree::new("new-file");
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);
    let before = lines(&editor);
    assert_eq!(line_at_point(&editor), "sub");

    press(&mut editor, "c");
    assert!(!editor.window_buffer().is_read_only());
    assert_eq!(editor.window_buffer().point_line(), 2);
    type_text(&mut editor, "notes.md");
    let table = editor.table(0..20).unwrap();
    assert_eq!(table.rows[2].cells[0], "notes.md");
    assert_eq!(table.rows[2].field.as_ref().unwrap().original, None);
    assert!(table.sort.is_empty() && table.preview.is_none());
    assert_eq!(table.hints[0], ("↵", "Create"));

    press(&mut editor, "RET");
    assert!(tree.0.join("notes.md").is_file());
    assert!(editor.window_buffer().is_read_only());
    wait_until(&mut editor, |editor| line_at_point(editor) == "notes.md");
    assert_eq!(lines(&editor).len(), before.len() + 1);
}

#[test]
fn a_name_already_listed_is_refused_and_the_row_stays() {
    let tree = Tree::new("new-file-exists");
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);

    press(&mut editor, "c");
    type_text(&mut editor, "LIB.rs");
    press(&mut editor, "RET");
    assert_eq!(editor.echo().as_deref(), Some("lib.rs already exists"));
    assert!(!editor.window_buffer().is_read_only());
    assert_eq!(line_at_point(&editor), "LIB.rs");

    press(&mut editor, "C-a C-k");
    type_text(&mut editor, "a/b");
    press(&mut editor, "RET");
    assert_eq!(
        editor.echo().as_deref(),
        Some("Cannot create a/b: the name contains /")
    );
}

#[test]
fn the_name_stays_on_its_line_and_cancelling_removes_the_row() {
    let tree = Tree::new("new-file-cancel");
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);
    let before = lines(&editor);

    press(&mut editor, "c");
    type_text(&mut editor, "x");
    let typed = lines(&editor);
    press(&mut editor, "C-a DEL");
    assert_eq!(lines(&editor), typed);
    assert_eq!(editor.echo().as_deref(), Some("The name stays on one line"));
    press(&mut editor, "C-n");
    assert_eq!(line_at_point(&editor), "x");

    press(&mut editor, "C-g");
    assert_eq!(lines(&editor), before);
    assert_eq!(line_at_point(&editor), "sub");
    assert!(editor.window_buffer().is_read_only());
    assert!(!tree.0.join("x").exists());

    press(&mut editor, "c RET");
    assert_eq!(lines(&editor), before);
}

/// Types `text` into the prompt, a key per character.
fn type_text(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        press(editor, &character.to_string());
    }
}

fn wait_for_message(editor: &mut Editor, start: &str) -> String {
    wait_until(editor, |editor| {
        editor.echo().is_some_and(|echo| echo.starts_with(start))
    });
    editor.echo().unwrap()
}

#[test]
fn copying_a_file_to_a_new_name_keeps_its_modified_time() {
    let tree = Tree::new("copy");
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);

    press(&mut editor, "n n C");
    let minibuffer = editor.minibuffer().unwrap();
    assert_eq!(minibuffer.prompt().subject, "lib.rs");
    assert_eq!(minibuffer.prompt().items_summary, "1 · 13 B");
    type_text(&mut editor, "copy.rs");
    let minibuffer = editor.minibuffer().unwrap();
    assert_eq!(minibuffer.note(), Some("New name"));
    assert_eq!(minibuffer.candidates()[0].verb, Some("Copy as"));
    assert_eq!(minibuffer.hints()[0], ("↵", "Copy"));

    press(&mut editor, "RET");
    wait_for_message(&mut editor, "Copied lib.rs to");
    let copy = tree.0.join("copy.rs");
    assert_eq!(fs::read_to_string(&copy).unwrap(), "fn main() {}\n");
    let modified = |path: PathBuf| fs::metadata(path).unwrap().modified().unwrap();
    assert_eq!(modified(copy), modified(tree.0.join("lib.rs")));
    wait_until(&mut editor, |editor| {
        lines(editor).contains(&"copy.rs".to_owned())
    });
}

#[test]
fn moving_marked_files_into_a_new_folder_takes_their_buffers_along() {
    let tree = Tree::new("move");
    let mut editor = editor();
    editor.find_file_noselect(&tree.0.join("lib.rs")).unwrap();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);

    press(&mut editor, "n n m m R");
    let minibuffer = editor.minibuffer().unwrap();
    assert_eq!(minibuffer.prompt().subject, "2 marked files");
    type_text(&mut editor, "dest/");
    let minibuffer = editor.minibuffer().unwrap();
    assert_eq!(minibuffer.note(), Some("New folder"));
    assert_eq!(minibuffer.candidates()[0].verb, Some("Create"));
    assert_eq!(minibuffer.hints()[0], ("↵", "Move"));
    press(&mut editor, "M-m");
    assert_eq!(editor.minibuffer().unwrap().hints()[0], ("↵", "Copy"));
    press(&mut editor, "M-m RET");

    wait_for_message(&mut editor, "Moved 2 files to");
    let destination = tree.0.join("dest");
    assert!(destination.join("lib.rs").exists() && destination.join("lib.rs~").exists());
    assert!(!tree.0.join("lib.rs").exists());
    assert!(
        editor
            .buffers()
            .iter()
            .any(|buffer| buffer.path() == Some(destination.join("lib.rs").as_path()))
    );
    wait_until(&mut editor, |editor| {
        !lines(editor).contains(&"lib.rs".to_owned())
    });
}

#[test]
fn moving_updates_references_first_and_backends_hear_of_the_move() {
    let tree = Tree::new("move-references");
    let refs = tree.0.join("refs.txt");
    fs::write(&refs, "use lib;\n").unwrap();
    let mut editor = editor();
    let buffer = editor.find_file_noselect(&refs).unwrap();
    buffer.set_point(buffer.text().len());
    buffer.insert("// unsaved\n");
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);

    press(&mut editor, "n n R");
    type_text(&mut editor, "sub/");
    press(&mut editor, "RET");
    let request = take_rename_requests(&mut editor).pop().unwrap();
    let (from, to) = (tree.0.join("lib.rs"), tree.0.join("sub/lib.rs"));
    assert_eq!(request.renames, [(from.clone(), to.clone())]);
    editor.refresh();
    assert_eq!(
        editor.echo().as_deref(),
        Some("Finding references to update…")
    );
    assert!(from.exists());

    let edits = FileEdits {
        path: refs.clone(),
        edits: vec![(4..7, "sub::lib".into())],
    };
    deliver_rename_edits(&mut editor, request.id, vec![edits]);
    wait_for_message(&mut editor, "Moved lib.rs to");
    assert!(editor.echo().unwrap().ends_with("; updated refs.txt"));
    assert_eq!(
        fs::read_to_string(&refs).unwrap(),
        "use sub::lib;\n// unsaved\n"
    );
    assert!(to.exists() && !from.exists());
    assert_eq!(editor.take_moved_files(), [(from, to)]);
}

#[test]
fn files_already_there_are_kept_unless_overwriting() {
    let tree = Tree::new("overwrite");
    fs::write(tree.0.join("sub/lib.rs"), "old").unwrap();
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);

    press(&mut editor, "n n C");
    type_text(&mut editor, "sub/");
    press(&mut editor, "RET");
    wait_for_message(&mut editor, "Nothing to copy");
    assert_eq!(
        fs::read_to_string(tree.0.join("sub/lib.rs")).unwrap(),
        "old"
    );

    press(&mut editor, "C");
    type_text(&mut editor, "sub/");
    press(&mut editor, "M-o RET");
    wait_for_message(&mut editor, "Copied lib.rs to");
    assert_eq!(
        fs::read_to_string(tree.0.join("sub/lib.rs")).unwrap(),
        "fn main() {}\n"
    );

    press(&mut editor, "M-< n R");
    type_text(&mut editor, "sub/inside/");
    press(&mut editor, "RET");
    assert!(editor.echo().unwrap().contains("onto or into itself"));
}

/// A listing of a fresh tree with its names editable, point on `sub`.
fn editing(name: &str) -> (Tree, Editor) {
    let tree = Tree::new(name);
    let mut editor = editor();
    open_directory(&mut editor, &tree.0, None, false).unwrap();
    wait_for_listing(&mut editor);
    press(&mut editor, "w");
    (tree, editor)
}

fn changes(editor: &Editor) -> Changes {
    editor.table(0..20).unwrap().changes.unwrap()
}

#[test]
fn edited_names_are_fields_and_applying_renames_their_files() {
    let (tree, mut editor) = editing("wdired");
    editor.find_file_noselect(&tree.0.join("lib.rs")).unwrap();
    assert!(!editor.window_buffer().is_read_only());
    assert_eq!(line_at_point(&editor), "sub");

    press(&mut editor, "TAB TAB C-a");
    type_text(&mut editor, "new_");
    assert_eq!(line_at_point(&editor), "new_lib.rs");
    let table = editor.table(0..20).unwrap();
    assert_eq!(table.notice.unwrap().label, "Editing names");
    assert!(table.sort.is_empty() && table.preview.is_none());
    assert_eq!(table.rows[0].field, None);
    assert_eq!(table.rows[3].cells[0], "new_lib.rs");
    assert_eq!(
        table.rows[3].field.as_ref().unwrap().original.as_deref(),
        Some("lib.rs")
    );
    let changes = changes(&editor);
    assert_eq!(changes.summary, "1 rename");
    assert_eq!(changes.edits, [("lib.rs".into(), "new_lib.rs".into())]);
    assert_eq!(changes.elsewhere, Elsewhere::Nothing);
    let status = editor.status_segments();
    assert_eq!(status[0].items[0].text, "Editing · 1 change");
    assert_eq!(status[0].items[0].tone, Tone::Warning);

    press(&mut editor, "C-c C-c");
    assert!(editor.window_buffer().is_read_only());
    assert_eq!(line_at_point(&editor), "new_lib.rs");
    wait_for_message(&mut editor, "Renamed lib.rs to new_lib.rs");
    assert!(tree.0.join("new_lib.rs").exists() && !tree.0.join("lib.rs").exists());
    assert!(
        editor
            .buffers()
            .iter()
            .any(|buffer| buffer.path() == Some(tree.0.join("new_lib.rs").as_path()))
    );
    wait_until(&mut editor, |editor| line_at_point(editor) == "new_lib.rs");
}

#[test]
fn names_stay_one_per_line_and_the_parent_stays_as_it_is() {
    let (_tree, mut editor) = editing("wdired-lines");
    let before = lines(&editor);
    press(&mut editor, "C-e C-d");
    assert_eq!(lines(&editor), before);
    assert_eq!(editor.echo().as_deref(), Some("Names stay one per line"));

    press(&mut editor, "C-k C-k C-y");
    assert_eq!(lines(&editor), before);

    press(&mut editor, "M-<");
    type_text(&mut editor, "x");
    assert_eq!(lines(&editor), before);
    assert_eq!(editor.echo().as_deref(), Some(".. cannot be renamed"));
}

#[test]
fn swapped_names_pass_through_temporary_names() {
    let (tree, mut editor) = editing("wdired-swap");
    fs::write(tree.0.join("lib.rs~"), "backup").unwrap();
    press(&mut editor, "C-n C-n C-e");
    type_text(&mut editor, "~");
    press(&mut editor, "C-n C-e DEL");
    assert_eq!(changes(&editor).summary, "2 renames");

    press(&mut editor, "C-c C-c");
    wait_for_message(&mut editor, "Renamed 2 files");
    assert_eq!(
        fs::read_to_string(tree.0.join("lib.rs~")).unwrap(),
        "fn main() {}\n"
    );
    assert_eq!(fs::read_to_string(tree.0.join("lib.rs")).unwrap(), "backup");
}

#[test]
fn names_that_cannot_be_are_refused_and_editing_goes_on() {
    let (_tree, mut editor) = editing("wdired-refuse");
    press(&mut editor, "C-n C-n C-a C-k");
    type_text(&mut editor, "sub");
    press(&mut editor, "C-c C-c");
    assert_eq!(
        editor.echo().as_deref(),
        Some("Two files would be named sub")
    );

    press(&mut editor, "C-a C-k C-c C-c");
    assert_eq!(
        editor.echo().as_deref(),
        Some("Cannot rename lib.rs: the new name is empty")
    );
    assert!(!editor.window_buffer().is_read_only());
}

#[test]
fn cancelling_puts_the_names_back() {
    let (_tree, mut editor) = editing("wdired-cancel");
    let before = lines(&editor);
    type_text(&mut editor, "renamed");
    press(&mut editor, "C-c C-k");
    assert_eq!(lines(&editor), before);
    assert!(editor.window_buffer().is_read_only());
    assert_eq!(editor.echo().as_deref(), Some("Edits discarded"));
    assert!(editor.table(0..20).unwrap().changes.is_none());
}

#[test]
fn replacing_in_names_changes_every_name_and_undoes_at_once() {
    let (_tree, mut editor) = editing("wdired-replace");
    press(&mut editor, "M-%");
    type_text(&mut editor, "lib");
    press(&mut editor, "RET");
    type_text(&mut editor, "core");
    press(&mut editor, "RET");
    assert_eq!(
        lines(&editor),
        ["..", "sub", ".hidden", "core.rs", "core.rs~"]
    );
    assert_eq!(editor.echo().as_deref(), Some("Replaced 2 occurrences"));

    press(&mut editor, "C-/");
    assert_eq!(
        lines(&editor),
        ["..", "sub", ".hidden", "lib.rs", "lib.rs~"]
    );
}

#[test]
fn references_a_backend_finds_are_shown_and_updated_with_the_rename() {
    let (tree, mut editor) = editing("wdired-references");
    let refs = tree.0.join("refs.txt");
    fs::write(&refs, "use lib;\nfn x() {}\n").unwrap();
    press(&mut editor, "TAB TAB C-a M-d");
    type_text(&mut editor, "core");

    // Asked again as each letter was typed; only the newest question counts.
    let request = take_rename_requests(&mut editor).pop().unwrap();
    assert_eq!(
        request.renames,
        [(tree.0.join("lib.rs"), tree.0.join("core.rs"))]
    );
    assert_eq!(changes(&editor).elsewhere, Elsewhere::Checking);
    press(&mut editor, "C-c C-c");
    assert!(
        editor
            .echo()
            .unwrap()
            .starts_with("Still finding references")
    );

    let edits = FileEdits {
        path: refs.clone(),
        edits: vec![(4..7, "core".into())],
    };
    deliver_rename_edits(&mut editor, request.id, vec![edits]);
    let changes = changes(&editor);
    assert_eq!(
        changes.elsewhere,
        Elsewhere::Files(vec![FileDiff {
            name: "refs.txt".into(),
            lines: vec![
                DiffLine::Removed("use lib;".into()),
                DiffLine::Added("use core;".into())
            ],
        }])
    );
    assert!(changes.options[0].selected);

    press(&mut editor, "C-c C-c");
    wait_for_message(&mut editor, "Renamed lib.rs to core.rs; updated refs.txt");
    assert_eq!(fs::read_to_string(&refs).unwrap(), "use core;\nfn x() {}\n");
    assert!(tree.0.join("core.rs").exists());
}

#[test]
fn references_are_left_alone_when_switched_off() {
    let (tree, mut editor) = editing("wdired-no-references");
    let refs = tree.0.join("refs.txt");
    fs::write(&refs, "use lib;\n").unwrap();
    press(&mut editor, "TAB TAB C-a M-d");
    type_text(&mut editor, "core");
    let request = take_rename_requests(&mut editor).pop().unwrap();
    let edits = FileEdits {
        path: refs.clone(),
        edits: vec![(4..7, "core".into())],
    };
    deliver_rename_edits(&mut editor, request.id, vec![edits]);
    editor.run_command("wdired-toggle-reference-updates");
    let changes = changes(&editor);
    assert_eq!(changes.elsewhere, Elsewhere::Nothing);
    assert!(!changes.options[0].selected);

    press(&mut editor, "C-c C-c");
    wait_for_message(&mut editor, "Renamed lib.rs to core.rs");
    assert_eq!(fs::read_to_string(&refs).unwrap(), "use lib;\n");
}
