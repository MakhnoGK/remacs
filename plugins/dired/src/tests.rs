use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{Editor, RowFace, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_files::FilesPlugin;
use remacs_minibuffer::MinibufferPlugin;

use crate::DiredPlugin;
use crate::commands::open_directory;
use crate::entry::{
    Entry, LocalTime, PARENT, SortKey, date_label, items_label, mode_label, size_label, sort,
};
use crate::listing::{DIRED_MODE, Listing};

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
    assert_eq!(table.rows[1].cells[..2], ["sub/", "2 items"]);
    assert_eq!(table.rows[1].face, RowFace::Directory);
    assert_eq!(table.rows[2].face, RowFace::Dim);
    assert_eq!(table.rows[3].cells[1], "13 B");
    assert_eq!(table.rows[3].cells[3], "-rw-r--r--");
    assert_eq!(
        table.crumbs.last().unwrap().label,
        editor.window_buffer().name()
    );
    let status = editor.status_segments();
    assert_eq!(status[0].items[0].1, "4 items");

    press(&mut editor, "x");
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
