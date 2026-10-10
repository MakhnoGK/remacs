use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use remacs_core::{Editor, Key, SCRATCH_BUFFER, Settings, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;

use crate::FilesPlugin;

const INDEX_TIMEOUT: Duration = Duration::from_secs(10);

/// A throwaway Git work tree with a few files.
struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("remacs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (path, contents) in [
            (".git/HEAD", ""),
            (".gitignore", ".env\n*.log\n"),
            (".env", "KEY=1\n"),
            ("node_modules/pkg/index.js", ""),
            ("Cargo.toml", "[package]\n"),
            ("src/marker.rs", ""),
            ("src/mark_ring.rs", ""),
            ("docs/marks.md", ""),
            ("target/marked.rs", ""),
        ] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        Self(root.canonicalize().unwrap())
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn editor_visiting(path: &Path) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &FilesPlugin]);
    editor.visit_file(path).unwrap();
    editor
}

/// Opens find-file for the first time, once the background index has scanned the project;
/// the receiver reports later index updates.
fn open_find_file(editor: &mut Editor) -> Receiver<()> {
    let (sender, notifications) = mpsc::channel();
    editor.set_notifier(Arc::new(move || {
        let _ = sender.send(());
    }));
    press(editor, "C-x C-f");
    notifications
        .recv_timeout(INDEX_TIMEOUT)
        .expect("the index finishes its first scan");
    editor.refresh();
    notifications
}

fn candidate_values(editor: &Editor) -> BTreeSet<String> {
    let minibuffer = editor.minibuffer().unwrap();
    minibuffer
        .candidates()
        .iter()
        .map(|candidate| candidate.value.clone())
        .collect()
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn type_text(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        editor.handle_key(Key::parse(&character.to_string()).unwrap());
    }
}

fn labels(editor: &Editor) -> Vec<String> {
    let minibuffer = editor.minibuffer().unwrap();
    minibuffer
        .candidates()
        .iter()
        .map(|candidate| format!("{} {}", candidate.label, candidate.annotation))
        .collect()
}

fn buffer_names(editor: &Editor) -> Vec<&str> {
    editor
        .buffers()
        .iter()
        .map(|buffer| buffer.name())
        .collect()
}

#[test]
fn find_file_matches_project_paths_fuzzily() {
    let project = Project::new("find-file");
    let mut editor = editor_visiting(&project.path("Cargo.toml"));
    open_find_file(&mut editor);
    type_text(&mut editor, "mark");
    assert_eq!(
        labels(&editor),
        ["docs/marks.md ", "src/marker.rs ", "src/mark_ring.rs "]
    );
    press(&mut editor, "C-n RET");
    assert_eq!(editor.window_buffer().name(), "marker.rs");

    press(&mut editor, "C-x C-f");
    type_text(&mut editor, "smaring");
    assert_eq!(labels(&editor), ["src/mark_ring.rs "]);
    assert_eq!(
        editor.minibuffer().unwrap().candidates()[0].matches,
        [0..1, 4..6, 9..13]
    );

    press(&mut editor, "C-a C-k");
    type_text(&mut editor, ".rs$ ^src");
    let minibuffer = editor.minibuffer().unwrap();
    assert_eq!(minibuffer.total(), 2);
    assert_eq!(minibuffer.candidates()[0].tag, Some("open"));
}

#[test]
fn find_file_browses_absolute_paths_and_creates_new_files() {
    let project = Project::new("browse");
    let mut editor = editor_visiting(&project.path("Cargo.toml"));
    press(&mut editor, "C-x C-f");
    type_text(&mut editor, &format!("{}/sr", project.0.display()));
    assert_eq!(labels(&editor), ["src/ "]);
    press(&mut editor, "TAB");
    assert_eq!(labels(&editor), ["mark_ring.rs ", "marker.rs "]);
    type_text(&mut editor, "new.rs");
    press(&mut editor, "RET");
    assert_eq!(editor.window_buffer().name(), "new.rs");
    assert_eq!(editor.echo().as_deref(), Some("(New file)"));
    type_text(&mut editor, "fn main() {}");
    press(&mut editor, "C-x C-s");
    assert_eq!(
        fs::read_to_string(project.path("src/new.rs")).unwrap(),
        "fn main() {}"
    );
}

#[test]
fn file_index_follows_file_system_changes() {
    let project = Project::new("index");
    let mut editor = editor_visiting(&project.path("Cargo.toml"));
    let notifications = open_find_file(&mut editor);
    fs::create_dir_all(project.path("lib/nested")).unwrap();
    for created in [
        "src/added.rs",
        "src/debug.log",
        "target/built.rs",
        "lib/nested/deep.rs",
    ] {
        fs::write(project.path(created), "").unwrap();
    }
    fs::remove_file(project.path("docs/marks.md")).unwrap();

    let expected: BTreeSet<String> = [
        ".env",
        ".gitignore",
        "Cargo.toml",
        "src/marker.rs",
        "src/mark_ring.rs",
        "src/added.rs",
        "src/debug.log",
        "lib/nested/deep.rs",
    ]
    .map(String::from)
    .into();
    let deadline = Instant::now() + INDEX_TIMEOUT;
    while candidate_values(&editor) != expected && Instant::now() < deadline {
        let _ = notifications.recv_timeout(deadline - Instant::now());
        editor.refresh();
    }
    assert_eq!(candidate_values(&editor), expected);
}

#[test]
fn the_settings_say_what_is_left_out_and_gitignore_only_when_asked() {
    let project = Project::new("index-settings");
    let mut editor = editor_visiting(&project.path("Cargo.toml"));
    let files_toml = "ignore = [\"docs/\", \"node_modules/\"]\nuse-gitignore = true\n";
    let settings =
        [("files.toml", files_toml)].map(|(name, text)| (PathBuf::from(name), text.to_owned()));
    editor.set_settings(Settings::from_files(settings));
    open_find_file(&mut editor);
    let expected: BTreeSet<String> = [
        ".gitignore",
        "Cargo.toml",
        "src/marker.rs",
        "src/mark_ring.rs",
        "target/marked.rs",
    ]
    .map(String::from)
    .into();
    assert_eq!(candidate_values(&editor), expected);
}

#[test]
fn the_scan_is_cached_and_a_stale_cache_gives_way_to_the_next_scan() {
    let project = Project::new("index-cache");
    let cache = std::env::temp_dir().join(format!("remacs-cache-{}", std::process::id()));
    let _ = fs::remove_dir_all(&cache);
    let mut first = editor_visiting(&project.path("Cargo.toml"));
    first.keep_caches_in(cache.clone());
    open_find_file(&mut first);
    let directory = first.project_cache_dir(&project.0).unwrap();
    let cached = || {
        fs::read_dir(&directory)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.extension().is_none())
    };
    let deadline = Instant::now() + INDEX_TIMEOUT;
    while cached().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let cached = cached().unwrap();
    assert_eq!(
        fs::read_to_string(&cached).unwrap(),
        ".env\n.gitignore\nCargo.toml\ndocs/marks.md\nsrc/mark_ring.rs\nsrc/marker.rs\n"
    );

    fs::write(&cached, "gone.rs\n").unwrap();
    let mut second = editor_visiting(&project.path("Cargo.toml"));
    second.keep_caches_in(cache.clone());
    let notifications = open_find_file(&mut second);
    let scanned: BTreeSet<String> = [
        ".env",
        ".gitignore",
        "Cargo.toml",
        "docs/marks.md",
        "src/mark_ring.rs",
        "src/marker.rs",
    ]
    .map(String::from)
    .into();
    let deadline = Instant::now() + INDEX_TIMEOUT;
    while candidate_values(&second) != scanned && Instant::now() < deadline {
        let _ = notifications.recv_timeout(deadline - Instant::now());
        second.refresh();
    }
    assert_eq!(candidate_values(&second), scanned);
    // The index may still be writing the cache it just published.
    let _ = fs::remove_dir_all(cache);
}

#[test]
fn write_file_visits_the_new_file() {
    let project = Project::new("write-file");
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &FilesPlugin]);
    type_text(&mut editor, "notes");
    press(&mut editor, "C-x C-s");
    assert_eq!(editor.minibuffer().unwrap().prompt().label, "Write file");
    press(&mut editor, "C-a C-k");
    type_text(&mut editor, &format!("{}/notes.md", project.0.display()));
    press(&mut editor, "M-RET");
    assert_eq!(editor.window_buffer().name(), "notes.md");
    assert_eq!(
        editor.window_buffer().path(),
        Some(project.path("notes.md").as_path())
    );
    assert!(!editor.window_buffer().is_modified());
    assert_eq!(
        fs::read_to_string(project.path("notes.md")).unwrap(),
        "notes"
    );
    assert!(!buffer_names(&editor).contains(&SCRATCH_BUFFER));

    press(&mut editor, "C-x C-w C-a C-k");
    type_text(&mut editor, &format!("{}/src/", project.0.display()));
    press(&mut editor, "M-RET");
    assert_eq!(
        fs::read_to_string(project.path("src/notes.md")).unwrap(),
        "notes"
    );
    assert_eq!(buffer_names(&editor), ["notes.md"]);

    press(&mut editor, "C-x C-w C-a C-k");
    type_text(&mut editor, &format!("{}/Cargo.toml", project.0.display()));
    press(&mut editor, "M-RET");
    assert!(
        editor
            .minibuffer()
            .unwrap()
            .prompt()
            .label
            .ends_with("Cargo.toml exists; overwrite? (y or n)")
    );
    press(&mut editor, "n");
    assert_eq!(editor.window_buffer().name(), "notes.md");
    assert_eq!(
        fs::read_to_string(project.path("Cargo.toml")).unwrap(),
        "[package]\n"
    );
    press(&mut editor, "C-x C-w C-a C-k");
    type_text(&mut editor, &format!("{}/Cargo.toml", project.0.display()));
    press(&mut editor, "M-RET y");
    assert_eq!(editor.window_buffer().name(), "Cargo.toml");
    assert_eq!(
        fs::read_to_string(project.path("Cargo.toml")).unwrap(),
        "notes"
    );
}

#[test]
fn buffers_switch_and_kill_with_confirmation() {
    let project = Project::new("buffers");
    let mut editor = editor_visiting(&project.path("src/marker.rs"));
    editor.visit_file(&project.path("Cargo.toml")).unwrap();
    assert_eq!(
        buffer_names(&editor),
        ["Cargo.toml", "marker.rs", "*scratch*"]
    );
    press(&mut editor, "C-x b");
    assert_eq!(labels(&editor)[0], "Cargo.toml ./");
    press(&mut editor, "RET");
    assert_eq!(
        buffer_names(&editor),
        ["marker.rs", "Cargo.toml", "*scratch*"]
    );

    type_text(&mut editor, "x");
    press(&mut editor, "C-x k RET");
    assert!(editor.minibuffer().unwrap().is_question());
    press(&mut editor, "n");
    assert_eq!(buffer_names(&editor).len(), 3);
    press(&mut editor, "C-x k RET y");
    assert_eq!(buffer_names(&editor), ["Cargo.toml", "*scratch*"]);

    press(&mut editor, "C-x b C-n C-x k");
    assert_eq!(labels(&editor), ["Cargo.toml ./"]);
}

#[test]
fn quitting_offers_to_save_each_modified_file() {
    let project = Project::new("quit");
    let mut editor = editor_visiting(&project.path("Cargo.toml"));
    type_text(&mut editor, "#");
    press(&mut editor, "C-x C-c");
    assert!(
        editor
            .minibuffer()
            .unwrap()
            .prompt()
            .label
            .starts_with("Save file")
    );
    press(&mut editor, "n");
    assert_eq!(
        editor.minibuffer().unwrap().prompt().label,
        "Modified buffers exist; exit anyway? (y or n)"
    );
    press(&mut editor, "C-g");
    assert!(!editor.quit_requested());

    press(&mut editor, "C-x C-c y");
    assert!(editor.quit_requested());
    assert_eq!(
        fs::read_to_string(project.path("Cargo.toml")).unwrap(),
        "#[package]\n"
    );
}
