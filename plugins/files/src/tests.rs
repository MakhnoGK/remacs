use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use remacs_core::{Editor, Key, Plugin, Registry, SCRATCH_BUFFER, Settings, parse_sequence};
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
    // The notifier is shared: auto-revert's file watcher may wake it before the index.
    let deadline = Instant::now() + INDEX_TIMEOUT;
    while candidate_values(editor).is_empty() {
        notifications
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the index finishes its first scan");
        editor.refresh();
    }
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
            .ends_with("Cargo.toml` exists; overwrite?")
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
fn without_the_prompt_kill_buffer_kills_the_current_buffer() {
    let project = Project::new("kill-current");
    let mut editor = editor_visiting(&project.path("src/marker.rs"));
    editor.visit_file(&project.path("Cargo.toml")).unwrap();
    let settings = [("files.toml", "kill-buffer-prompt = false\n")]
        .map(|(name, text)| (PathBuf::from(name), text.to_owned()));
    editor.set_settings(Settings::from_files(settings));

    press(&mut editor, "C-x k");
    assert!(editor.minibuffer().is_none());
    assert_eq!(buffer_names(&editor), ["marker.rs", "*scratch*"]);

    type_text(&mut editor, "x");
    press(&mut editor, "C-x k");
    assert!(editor.minibuffer().unwrap().is_question());
    press(&mut editor, "y");
    assert_eq!(buffer_names(&editor), ["*scratch*"]);
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
        "Modified buffers exist; exit anyway?"
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

/// A terminal running something other than its shell.
struct RunningPlugin;

impl Plugin for RunningPlugin {
    fn register(&self, registry: &mut Registry) {
        registry
            .quit_queries
            .push(|_| vec!["*term: vim*".to_owned()]);
    }
}

#[test]
fn quitting_with_nothing_to_lose_asks_nothing() {
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &FilesPlugin]);
    press(&mut editor, "s-q");
    assert!(editor.minibuffer().is_none());
    assert!(editor.quit_requested());
}

#[test]
fn quitting_asks_before_ending_running_programs() {
    let project = Project::new("quit-running");
    let mut editor = Editor::new(&[
        &EditingPlugin,
        &MinibufferPlugin,
        &FilesPlugin,
        &RunningPlugin,
    ]);
    editor.visit_file(&project.path("Cargo.toml")).unwrap();
    type_text(&mut editor, "#");
    let question = "Programs run in `*term: vim*`; kill them and exit anyway?";

    press(&mut editor, "C-x C-c n y");
    assert_eq!(editor.minibuffer().unwrap().prompt().label, question);
    press(&mut editor, "n");
    assert!(!editor.quit_requested());

    press(&mut editor, "s-q n y");
    assert_eq!(editor.minibuffer().unwrap().prompt().label, question);
    press(&mut editor, "y");
    assert!(editor.quit_requested());
}

fn contents(editor: &Editor) -> String {
    let buffer = editor.window_buffer();
    buffer.contents(0..buffer.text().len())
}

fn prompt_label(editor: &Editor) -> String {
    editor.minibuffer().unwrap().prompt().label.to_string()
}

#[test]
fn an_unmodified_buffer_follows_its_file_and_only_its_changes_wake_the_editor() {
    let project = Project::new("follow");
    let path = project.path("Cargo.toml");
    fs::write(&path, "[package]\nname = \"one\"\n").unwrap();
    let mut editor = editor_visiting(&path);
    let wakeups = Arc::new(AtomicUsize::new(0));
    let (sender, notifications) = mpsc::channel();
    let counted = Arc::clone(&wakeups);
    editor.set_notifier(Arc::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        let _ = sender.send(());
    }));
    press(&mut editor, "C-n C-f C-f");
    // FSEvents may still deliver the write that made the file, from before the watch began.
    std::thread::sleep(Duration::from_millis(300));
    editor.refresh();
    wakeups.store(0, Ordering::SeqCst);

    for file in 0..20 {
        fs::write(project.path(&format!("other_{file}.txt")), "x").unwrap();
    }
    std::thread::sleep(Duration::from_millis(300));
    editor.refresh();
    assert_eq!(wakeups.load(Ordering::SeqCst), 0);

    let expected = "[package]\nname = \"two\"\n";
    fs::write(&path, expected).unwrap();
    let deadline = Instant::now() + INDEX_TIMEOUT;
    while contents(&editor) != expected && Instant::now() < deadline {
        let _ = notifications.recv_timeout(deadline - Instant::now());
        editor.refresh();
    }
    assert_eq!(contents(&editor), expected);
    let buffer = editor.window_buffer();
    assert!(!buffer.is_modified());
    assert_eq!((buffer.point_line(), buffer.point_column()), (1, 2));
    // One write is several events; a refresh landing between them may let a second wake.
    assert!(wakeups.load(Ordering::SeqCst) <= 2);
}

#[test]
fn visiting_a_file_changed_on_disk_reads_it_again() {
    let project = Project::new("revisit");
    let path = project.path("Cargo.toml");
    let mut editor = editor_visiting(&path);
    fs::write(&path, "[workspace]\n").unwrap();
    editor.visit_file(&path).unwrap();
    assert_eq!(contents(&editor), "[workspace]\n");
}

#[test]
fn a_modified_buffer_keeps_its_edits_and_saving_over_a_changed_file_asks() {
    let project = Project::new("superseded");
    let path = project.path("Cargo.toml");
    let mut editor = editor_visiting(&path);
    type_text(&mut editor, "#");
    fs::write(&path, "[workspace]\n").unwrap();
    editor.visit_file(&path).unwrap();
    assert_eq!(contents(&editor), "#[package]\n");
    assert!(editor.echo().unwrap().contains("changed on disk"));

    press(&mut editor, "C-x C-s");
    assert!(prompt_label(&editor).ends_with("Cargo.toml` changed on disk; save over it?"));
    press(&mut editor, "n");
    assert_eq!(fs::read_to_string(&path).unwrap(), "[workspace]\n");

    press(&mut editor, "C-x C-s y");
    assert_eq!(fs::read_to_string(&path).unwrap(), "#[package]\n");
    type_text(&mut editor, "#");
    press(&mut editor, "C-x C-s");
    assert!(editor.minibuffer().is_none());
    assert_eq!(fs::read_to_string(&path).unwrap(), "##[package]\n");
}

#[test]
fn saving_a_new_file_asks_if_someone_made_it_meanwhile() {
    let project = Project::new("made-meanwhile");
    let path = project.path("new.rs");
    let mut editor = editor_visiting(&path);
    type_text(&mut editor, "mine");
    fs::write(&path, "theirs").unwrap();
    press(&mut editor, "C-x C-s");
    assert!(prompt_label(&editor).ends_with("new.rs` changed on disk; save over it?"));
    press(&mut editor, "n");
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs");
}

#[test]
fn a_deleted_file_is_reported_and_saving_asks_before_writing_it_again() {
    let project = Project::new("deleted");
    let path = project.path("Cargo.toml");
    let mut editor = editor_visiting(&path);
    fs::remove_file(&path).unwrap();
    editor.visit_file(&path).unwrap();
    assert_eq!(contents(&editor), "[package]\n");
    assert!(editor.echo().unwrap().contains("its file was deleted"));
    editor.clear_message();
    editor.visit_file(&path).unwrap();
    assert_eq!(editor.echo(), None, "reported once");

    type_text(&mut editor, "#");
    press(&mut editor, "C-x C-s");
    assert!(prompt_label(&editor).ends_with("Cargo.toml` was deleted; save it again?"));
    press(&mut editor, "n");
    assert!(!path.exists());
    press(&mut editor, "C-x C-s y");
    assert_eq!(fs::read_to_string(&path).unwrap(), "#[package]\n");
}

fn editor_keeping_session(store: &Path, opening: Option<&Path>, restore: bool) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &FilesPlugin]);
    let settings = [("files.toml", format!("restore-session = {restore}\n"))]
        .map(|(name, text)| (PathBuf::from(name), text));
    editor.set_settings(Settings::from_files(settings));
    crate::keep_session_in(&mut editor, store.to_path_buf(), opening);
    editor
}

#[test]
fn quitting_keeps_the_open_files_which_the_next_start_reopens_where_they_were() {
    let project = Project::new("session");
    fs::write(project.path("src/marker.rs"), "one\ntwo\nthree\n").unwrap();
    let store = project.path("state/session.json");
    let mut editor = editor_keeping_session(&store, None, true);
    editor.visit_file(&project.path("Cargo.toml")).unwrap();
    editor.visit_file(&project.path("src/marker.rs")).unwrap();
    editor.set_point_at(2, 1);
    editor.window_buffer_mut().fold(Some(1..2));
    press(&mut editor, "s-q");
    assert!(editor.quit_requested());

    let editor = editor_keeping_session(&store, None, true);
    assert_eq!(
        buffer_names(&editor),
        ["marker.rs", "Cargo.toml", SCRATCH_BUFFER]
    );
    let shown = editor.window_buffer();
    assert_eq!((shown.point_line(), shown.point_column()), (2, 1));
    assert_eq!(shown.folded_lines(), vec![1..2], "its folds too");
    fs::write(project.path("src/marker.rs"), "one\ntwo\n").unwrap();
    let editor = editor_keeping_session(&store, None, true);
    assert!(
        editor.window_buffer().folded_lines().is_empty(),
        "not in a file with other lines since"
    );

    let other = Project::new("session-other");
    let editor = editor_keeping_session(&store, Some(&other.path("Cargo.toml")), true);
    assert_eq!(buffer_names(&editor), [SCRATCH_BUFFER]);
    let editor = editor_keeping_session(&store, None, false);
    assert_eq!(buffer_names(&editor), [SCRATCH_BUFFER]);

    fs::remove_file(project.path("Cargo.toml")).unwrap();
    let editor = editor_keeping_session(&store, Some(&project.0), true);
    assert_eq!(buffer_names(&editor), ["marker.rs", SCRATCH_BUFFER]);
    assert!(
        editor
            .message()
            .unwrap()
            .text
            .starts_with("Not reopened: ")
    );
}

#[test]
fn revert_buffer_reads_the_file_again_asking_first_when_modified() {
    let project = Project::new("revert");
    let path = project.path("Cargo.toml");
    let mut editor = editor_visiting(&path);
    fs::write(&path, "[workspace]\n").unwrap();
    editor.run_command("revert-buffer");
    assert_eq!(contents(&editor), "[workspace]\n");

    type_text(&mut editor, "#");
    editor.run_command("revert-buffer");
    assert!(prompt_label(&editor).starts_with("Discard edits and read"));
    press(&mut editor, "n");
    assert_eq!(contents(&editor), "#[workspace]\n");
    editor.run_command("revert-buffer");
    press(&mut editor, "y");
    assert_eq!(contents(&editor), "[workspace]\n");
    assert!(!editor.window_buffer().is_modified());
}
