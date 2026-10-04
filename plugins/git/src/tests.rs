use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{
    DiffRow, Editor, FactValue, FileState, FileStatus, LineCounts, OutlineRow, parse_sequence,
};
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

/// A bare repository the tests push to, removed with it.
struct Remote(PathBuf);

impl Remote {
    /// Pushes `main` of `repository` to a new remote, `origin`, whose default it is.
    fn for_repository(repository: &Repository) -> Self {
        let path = repository.0.with_extension("remote.git");
        let _ = fs::remove_dir_all(&path);
        output(
            git(&repository.0)
                .args(["init", "-q", "--bare", "-b", "main"])
                .arg(&path),
        )
        .unwrap();
        repository.git(&["remote", "add", "origin", path.to_str().unwrap()]);
        repository.git(&["push", "-q", "-u", "origin", "main"]);
        repository.git(&["remote", "set-head", "origin", "main"]);
        Self(path)
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn commit(repository: &Repository, name: &str, contents: &str, message: &str) {
    repository.write(name, contents);
    repository.git(&["add", name]);
    repository.git(&["commit", "-q", "-m", message]);
}

/// The status buffer of `repository`, once its status is shown.
fn status_buffer(repository: &Repository) -> Editor {
    let mut editor = editor();
    editor.visit_file(&repository.0.join("lib.rs")).unwrap();
    editor.run_command("git-status");
    assert_eq!(editor.window_buffer().mode(), "Git");
    wait_until(&mut editor, "the status", |editor| {
        editor
            .outline(0..100)
            .is_some_and(|outline| !outline.facts.is_empty())
    });
    editor
}

/// Each row in short: `> Staged 2`, `A src/x.rs +3`, `+line`.
fn rows(editor: &Editor) -> Vec<String> {
    editor
        .outline(0..100)
        .unwrap()
        .rows
        .into_iter()
        .map(|row| match row {
            OutlineRow::Heading {
                title, count, open, ..
            } => {
                format!("{} {title} {count}", if open { "v" } else { ">" })
            }
            OutlineRow::File {
                state,
                name,
                directory,
                lines,
                ..
            } => {
                let lines = lines.map_or(String::new(), |lines| {
                    format!(" +{} -{}", lines.added, lines.removed)
                });
                format!("{} {directory}{name}{lines}", state.letter())
            }
            OutlineRow::Diff(DiffRow::Hunk(text)) => text,
            OutlineRow::Diff(DiffRow::Added(text)) => format!("+{text}"),
            OutlineRow::Diff(DiffRow::Removed(text)) => format!("-{text}"),
            OutlineRow::Diff(DiffRow::Context(text)) => format!(" {text}"),
            OutlineRow::Item { name, description } => format!("{name} {description}"),
        })
        .collect()
}

fn facts(editor: &Editor) -> Vec<String> {
    editor
        .outline(0..100)
        .unwrap()
        .facts
        .into_iter()
        .map(|fact| {
            let value = match fact.value {
                FactValue::Ref(chip) => chip.name,
                FactValue::Code(text) => text,
            };
            format!("{}: {value} · {}", fact.label, fact.note)
        })
        .collect()
}

fn go_to_row(editor: &mut Editor, row: &str) {
    let line = rows(editor).iter().position(|each| each == row).unwrap();
    editor.set_point_at(line, 0);
}

#[test]
fn the_status_shows_the_branch_and_the_changes_by_section() {
    let repository = Repository::new("status");
    repository.write("lib.rs", "one\ntwo\n");
    repository.write("src/added.rs", "a\nb\n");
    repository.git(&["add", "src/added.rs"]);
    repository.write("notes/idea.md", "");
    repository.write("draft.md", "");
    let mut editor = status_buffer(&repository);

    wait_until(&mut editor, "the line counts", |editor| {
        rows(editor).contains(&"M ./lib.rs +1 -0".to_owned())
    });
    assert_eq!(
        rows(&editor),
        [
            "v Staged 1",
            "A src/added.rs +2 -0",
            "v Unstaged 1",
            "M ./lib.rs +1 -0",
            "v Untracked 2",
            "? ./draft.md",
            "? ./notes/",
        ]
    );
    assert_eq!(facts(&editor), ["Head: main · First"]);
    assert_eq!(
        rows(&editor)[editor.window_buffer().point_line()],
        "A src/added.rs +2 -0"
    );
    let pill: Vec<String> = editor
        .status_segments()
        .into_iter()
        .flat_map(|segment| segment.items)
        .map(|item| item.text)
        .collect();
    assert_eq!(pill, ["main"]);

    go_to_row(&mut editor, "v Untracked 2");
    press(&mut editor, "TAB");
    assert_eq!(rows(&editor).last().unwrap(), "> Untracked 2");
}

#[test]
fn upstream_and_base_say_how_far_the_branch_is() {
    let repository = Repository::new("base");
    let _remote = Remote::for_repository(&repository);
    repository.git(&["switch", "-q", "-c", "feature"]);
    commit(&repository, "feature.rs", "f\n", "Start the feature");
    repository.git(&["push", "-q", "-u", "origin", "feature"]);
    commit(&repository, "feature.rs", "f\ng\n", "Grow the feature");
    repository.git(&["switch", "-q", "main"]);
    commit(&repository, "main.rs", "m\n", "Move main on");
    repository.git(&["switch", "-q", "feature"]);
    let mut editor = status_buffer(&repository);

    assert_eq!(
        facts(&editor),
        [
            "Head: feature · Grow the feature",
            "Upstream: origin/feature · 1 commit to push",
            "Base: main · 1 new commit",
        ]
    );
    let pill: Vec<String> = editor
        .status_segments()
        .into_iter()
        .flat_map(|segment| segment.items)
        .map(|item| item.text)
        .collect();
    assert_eq!(pill, ["feature", "↑1 ↓0"]);

    repository.git(&["branch", "-q", "develop", "main~1"]);
    repository.git(&["config", "branch.feature.gh-merge-base", "develop"]);
    editor.run_command("git-refresh");
    wait_until(&mut editor, "the configured base", |editor| {
        facts(editor)[2] == "Base: develop · Up to date"
    });

    repository.write("feature.rs", "draft\n");
    repository.git(&["stash", "push", "-q", "-m", "Try a draft"]);
    wait_until(&mut editor, "the stash", |editor| {
        rows(editor) == ["> Stashes 1"]
    });
    editor.set_point_at(0, 0);
    press(&mut editor, "TAB");
    assert_eq!(
        rows(&editor),
        ["v Stashes 1", "stash@{0} On feature: Try a draft"]
    );
}

#[test]
fn hunks_and_files_are_staged_unstaged_and_discarded() {
    let repository = Repository::new("stage");
    let lines: String = (1..=20).map(|line| format!("line {line}\n")).collect();
    commit(&repository, "long.rs", &lines, "Long file");
    let changed = lines
        .replace("line 2\n", "line two\n")
        .replace("line 19\n", "line nineteen\n");
    repository.write("long.rs", &changed);
    let mut editor = status_buffer(&repository);
    wait_until(&mut editor, "the change", |editor| {
        rows(editor).contains(&"M ./long.rs +2 -2".to_owned())
    });

    go_to_row(&mut editor, "M ./long.rs +2 -2");
    press(&mut editor, "TAB");
    wait_until(&mut editor, "the diff", |editor| {
        rows(editor)
            .iter()
            .filter(|row| row.starts_with("@@"))
            .count()
            == 2
    });
    assert!(rows(&editor).contains(&"-line 2".to_owned()));
    assert!(rows(&editor).contains(&"+line two".to_owned()));

    let second_hunk = rows(&editor)
        .iter()
        .rposition(|row| row.starts_with("@@"))
        .unwrap();
    editor.set_point_at(second_hunk, 0);
    press(&mut editor, "s");
    wait_for_status(&mut editor, &repository, "MM long.rs\n");
    assert!(
        repository
            .git(&["diff", "--cached"])
            .contains("+line nineteen")
    );
    assert!(!repository.git(&["diff", "--cached"]).contains("+line two"));

    wait_until(&mut editor, "the staged file", |editor| {
        rows(editor).contains(&"M ./long.rs +1 -1".to_owned()) && rows(editor)[0] == "v Staged 1"
    });
    editor.set_point_at(1, 0);
    press(&mut editor, "u");
    wait_for_status(&mut editor, &repository, " M long.rs\n");

    wait_until(&mut editor, "the unstaged file", |editor| {
        rows(editor)
            .first()
            .is_some_and(|row| row == "v Unstaged 1")
    });
    go_to_row(&mut editor, "M ./long.rs +2 -2");
    press(&mut editor, "k");
    assert!(editor.minibuffer().is_some());
    press(&mut editor, "y");
    wait_for_status(&mut editor, &repository, "");
    assert_eq!(
        fs::read_to_string(repository.0.join("long.rs")).unwrap(),
        lines
    );
}

#[test]
fn visiting_a_diff_line_opens_the_file_there() {
    let repository = Repository::new("visit");
    let lines: String = (1..=10).map(|line| format!("line {line}\n")).collect();
    commit(&repository, "long.rs", &lines, "Long file");
    repository.write("long.rs", &lines.replace("line 7\n", "line seven\n"));
    let mut editor = status_buffer(&repository);
    wait_until(&mut editor, "the change", |editor| {
        rows(editor).contains(&"M ./long.rs +1 -1".to_owned())
    });
    go_to_row(&mut editor, "M ./long.rs +1 -1");
    press(&mut editor, "TAB");
    wait_until(&mut editor, "the diff", |editor| {
        rows(editor).contains(&"+line seven".to_owned())
    });
    go_to_row(&mut editor, "+line seven");
    press(&mut editor, "RET");
    let buffer = editor.window_buffer();
    assert_eq!(buffer.path(), Some(repository.0.join("long.rs").as_path()));
    assert_eq!(buffer.point_line(), 6);
}
