use std::fs;
use std::path::{Path, PathBuf};

use remacs_core::{Editor, Key, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;

use crate::FilesPlugin;

/// A throwaway Git work tree with a few files.
struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("remacs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (path, contents) in [
            (".git/HEAD", ""),
            (".gitignore", "target/\n"),
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
fn find_file_matches_project_files_by_name() {
    let project = Project::new("find-file");
    let mut editor = editor_visiting(&project.path("Cargo.toml"));
    press(&mut editor, "C-x C-f");
    type_text(&mut editor, "mark");
    assert_eq!(
        labels(&editor),
        ["marks.md docs/", "marker.rs src/", "mark_ring.rs src/"]
    );
    press(&mut editor, "C-n RET");
    assert_eq!(editor.window_buffer().name(), "marker.rs");

    press(&mut editor, "C-x C-f");
    type_text(&mut editor, "src/mark");
    let minibuffer = editor.minibuffer().unwrap();
    assert_eq!(minibuffer.total(), 2);
    assert_eq!(minibuffer.candidates()[0].tag, Some("open"));
    assert_eq!(minibuffer.candidates()[0].matches.first(), Some(&(0..8)));
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
