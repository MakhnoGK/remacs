use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{Editor, FileState, FileStatus, LineCounts, parse_sequence};
use remacs_dired::{DiredPlugin, list_directory};
use remacs_editing::EditingPlugin;
use remacs_files::FilesPlugin;
use remacs_minibuffer::MinibufferPlugin;

use crate::GitPlugin;
use crate::command::{git, output};

const TIMEOUT: Duration = Duration::from_secs(10);

/// A throwaway repository with `lib.rs` and `kept.rs` committed.
struct Repository(PathBuf);

impl Repository {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("remacs-git-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let repository = Self(root);
        repository.git(&["init", "-q", "-b", "main"]);
        repository.git(&["config", "user.email", "test@example.com"]);
        repository.git(&["config", "user.name", "Test"]);
        repository.write("lib.rs", "one\n");
        repository.write("kept.rs", "kept\n");
        repository.git(&["add", "."]);
        repository.git(&["commit", "-q", "-m", "First"]);
        repository
    }

    fn git(&self, args: &[&str]) -> String {
        String::from_utf8(output(git(&self.0).args(args)).unwrap()).unwrap()
    }

    fn write(&self, name: &str, contents: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn short_status(&self) -> String {
        self.git(&["status", "--porcelain", "--untracked-files=all"])
    }
}

impl Drop for Repository {
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
        &GitPlugin,
    ]);
    editor.viewport.set_height(20);
    editor
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn type_text(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        press(editor, &character.to_string());
    }
}

fn wait_until(editor: &mut Editor, what: &str, done: impl Fn(&Editor) -> bool) {
    let start = Instant::now();
    while !done(editor) {
        assert!(start.elapsed() < TIMEOUT, "{what} never happened");
        thread::sleep(Duration::from_millis(10));
        editor.refresh();
    }
}

/// Refreshes until `git status --porcelain` says `expected`, as background jobs finish.
fn wait_for_status(editor: &mut Editor, repository: &Repository, expected: &str) {
    let start = Instant::now();
    loop {
        let status = repository.short_status();
        if status == expected {
            return;
        }
        assert!(start.elapsed() < TIMEOUT, "status stayed {status:?}");
        thread::sleep(Duration::from_millis(10));
        editor.refresh();
    }
}

/// The listed names and their states' letters.
fn states(editor: &Editor) -> Vec<(String, String)> {
    let table = editor.table(0..20).unwrap();
    table
        .rows
        .into_iter()
        .map(|row| (row.cells[0].clone(), row.cells[1].clone()))
        .collect()
}

fn state_of(editor: &Editor, name: &str) -> String {
    states(editor)
        .into_iter()
        .find(|(listed, _)| listed == name)
        .map(|(_, state)| state)
        .unwrap_or_default()
}

fn list(editor: &mut Editor, directory: &Path) {
    list_directory(editor, directory).unwrap();
    wait_until(editor, "the listing", |editor| {
        editor
            .table(0..20)
            .is_some_and(|table| table.rows.len() > 2)
    });
}

fn go_to(editor: &mut Editor, name: &str) {
    let line = states(editor)
        .iter()
        .position(|(listed, _)| listed == name)
        .unwrap();
    editor.set_point_at(line, 0);
    editor.refresh();
}

#[test]
fn listings_show_each_files_state_and_the_preview_its_lines() {
    let repository = Repository::new("states");
    repository.write("lib.rs", "one\ntwo\n");
    repository.write("added.rs", "a\nb\nc\n");
    repository.git(&["add", "added.rs"]);
    repository.write("draft.md", "");
    repository.write("notes/idea.md", "");
    let mut editor = editor();
    list(&mut editor, &repository.0);

    wait_until(&mut editor, "states", |editor| {
        state_of(editor, "lib.rs") == "M"
    });
    assert_eq!(
        states(&editor),
        [
            ("..", ""),
            (".git/", ""),
            ("notes/", "?"),
            ("added.rs", "A"),
            ("draft.md", "?"),
            ("kept.rs", ""),
            ("lib.rs", "M"),
        ]
        .map(|(name, state)| (name.to_owned(), state.to_owned()))
    );
    let table = editor.table(0..20).unwrap();
    assert_eq!(table.rows[3].state, Some(FileState::Added));

    go_to(&mut editor, "lib.rs");
    let preview = editor.table(0..20).unwrap().preview.unwrap();
    let modified = FileStatus {
        state: FileState::Modified,
        lines: Some(LineCounts {
            added: 1,
            removed: 0,
        }),
    };
    assert_eq!(preview.version, Some(("Git", modified)));
    go_to(&mut editor, "..");
    assert_eq!(editor.table(0..20).unwrap().preview.unwrap().version, None);
}

#[test]
fn a_change_on_disk_shows_without_asking() {
    let repository = Repository::new("watch");
    let mut editor = editor();
    list(&mut editor, &repository.0);
    wait_until(&mut editor, "the first status", |editor| {
        editor.file_status(&repository.0.join("kept.rs")).is_none()
            && editor.version_control(&repository.0).is_some()
    });

    repository.write("kept.rs", "changed\n");
    wait_until(&mut editor, "the change", |editor| {
        state_of(editor, "kept.rs") == "M"
    });
    repository.git(&["add", "kept.rs"]);
    repository.git(&["commit", "-q", "-m", "Second"]);
    wait_until(&mut editor, "the commit", |editor| {
        state_of(editor, "kept.rs").is_empty()
    });
}

#[test]
fn moving_a_file_keeps_what_was_staged_as_git_mv_does() {
    let repository = Repository::new("move");
    repository.write("lib.rs", "staged\n");
    repository.git(&["add", "lib.rs"]);
    repository.write("lib.rs", "unstaged\n");
    let mut editor = editor();
    list(&mut editor, &repository.0);

    go_to(&mut editor, "lib.rs");
    press(&mut editor, "R");
    let shown = |editor: &Editor| -> Vec<String> {
        let prompt = editor.minibuffer().unwrap().prompt();
        prompt
            .options
            .iter()
            .filter(|option| option.is_shown())
            .map(|option| option.labels.join("/"))
            .collect()
    };
    assert_eq!(
        shown(&editor),
        ["Copy/Move", "Move with git mv", "Overwrite existing"]
    );
    press(&mut editor, "M-m");
    assert_eq!(shown(&editor), ["Copy/Move", "Overwrite existing"]);
    press(&mut editor, "M-m");
    type_text(&mut editor, "moved.rs");
    press(&mut editor, "RET");

    // Too unlike the committed file to count as a rename, as with git mv itself.
    wait_for_status(&mut editor, &repository, "D  lib.rs\nAM moved.rs\n");
    assert_eq!(repository.git(&["show", ":moved.rs"]), "staged\n");
}

#[test]
fn renaming_in_place_tells_git_unless_switched_off() {
    let repository = Repository::new("rename");
    let mut editor = editor();
    list(&mut editor, &repository.0);
    wait_until(&mut editor, "the repository", |editor| {
        editor.version_control(&repository.0).is_some()
    });

    let rename = |editor: &mut Editor, from: &str, to: &str| {
        go_to(editor, from);
        press(editor, "w C-k");
        type_text(editor, to);
    };
    rename(&mut editor, "lib.rs", "main.rs");
    let options = editor.table(0..20).unwrap().changes.unwrap().options;
    assert_eq!(options[0].label, "Rename with git mv");
    assert!(options[0].selected);
    press(&mut editor, "C-c C-c");
    wait_for_status(&mut editor, &repository, "R  lib.rs -> main.rs\n");

    wait_until(&mut editor, "the listing", |editor| {
        state_of(editor, "main.rs") == "R"
    });
    rename(&mut editor, "kept.rs", "other.rs");
    editor.run_command("wdired-toggle-move-recording");
    press(&mut editor, "C-c C-c");
    wait_for_status(
        &mut editor,
        &repository,
        " D kept.rs\nR  lib.rs -> main.rs\n?? other.rs\n",
    );
}
