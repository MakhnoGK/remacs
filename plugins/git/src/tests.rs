use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{
    ConsoleColor, ConsoleStyle, DiffRow, Editor, Face, Fact, FactValue, FileState, FileStatus,
    LineChange, LineCounts, OutlineRow, Settings, Tone, parse_sequence,
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
        let key = match character {
            ' ' => "SPC".to_owned(),
            other => other.to_string(),
        };
        press(editor, &key);
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

fn line_changes(editor: &Editor) -> Vec<(Range<usize>, LineChange)> {
    editor
        .line_changes(0..100)
        .into_iter()
        .map(|changed| (changed.lines, changed.change))
        .collect()
}

#[test]
fn buffers_mark_the_lines_changed_since_the_last_commit_saved_or_not() {
    let repository = Repository::new("line-changes");
    commit(&repository, "lib.rs", "one\ntwo\nthree\n", "Three lines");
    let mut editor = editor();
    editor.visit_file(&repository.0.join("lib.rs")).unwrap();
    editor.window_buffer_mut().insert("zero\n");
    wait_until(&mut editor, "the added line", |editor| {
        line_changes(editor) == [(0..1, LineChange::Added)]
    });

    editor.window_buffer_mut().save().unwrap();
    repository.git(&["commit", "-q", "-am", "Four lines"]);
    wait_until(&mut editor, "the new commit", |editor| {
        line_changes(editor).is_empty()
    });

    repository.write("new.rs", "a\nb\n");
    editor.visit_file(&repository.0.join("new.rs")).unwrap();
    wait_until(&mut editor, "the untracked file", |editor| {
        line_changes(editor) == [(0..2, LineChange::Added)]
    });
}

/// Each comparison used to wake the UI for a full refresh; typing on a line already
/// marked changes nothing in the gutter, so it wakes nothing.
#[test]
fn typing_on_a_changed_line_wakes_the_ui_for_no_comparison() {
    let repository = Repository::new("line-changes-quiet");
    let mut editor = editor();
    let wakes = Arc::new(AtomicUsize::new(0));
    editor.set_notifier(Arc::new({
        let wakes = Arc::clone(&wakes);
        move || {
            wakes.fetch_add(1, Ordering::Relaxed);
        }
    }));
    editor.visit_file(&repository.0.join("lib.rs")).unwrap();
    editor.window_buffer_mut().insert("zero\n");
    wait_until(&mut editor, "the added line", |editor| {
        line_changes(editor) == [(0..1, LineChange::Added)]
    });
    editor.window_buffer_mut().set_point(0);
    thread::sleep(Duration::from_millis(300));
    let before = wakes.load(Ordering::Relaxed);
    for character in "typed on the added line".chars() {
        editor.window_buffer_mut().insert(&character.to_string());
        editor.refresh();
        thread::sleep(Duration::from_millis(5));
    }
    thread::sleep(Duration::from_millis(300));
    assert_eq!(line_changes(&editor), [(0..1, LineChange::Added)]);
    assert_eq!(wakes.load(Ordering::Relaxed) - before, 0);
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
        .map(describe_row)
        .collect()
}

fn describe_row(row: OutlineRow) -> String {
    match row {
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
    }
}

fn facts(editor: &Editor) -> Vec<String> {
    editor
        .outline(0..100)
        .unwrap()
        .facts
        .into_iter()
        .map(describe_fact)
        .collect()
}

fn describe_fact(fact: Fact) -> String {
    let value = match fact.value {
        FactValue::Ref(chip) => chip.name,
        FactValue::Code(text) => text,
    };
    format!("{}: {value} · {}", fact.label, fact.note)
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
fn untracked_files_unfold_into_added_lines_and_directories_into_their_files() {
    let repository = Repository::new("untracked");
    repository.write("draft.md", "one\n");
    repository.write("notes/a.md", "alpha\n");
    repository.write("notes/deep/b.md", "beta\n");
    repository.write("big.txt", "0123456789ab");
    repository.write("data.bin", "\0\x01");
    let mut editor = editor();
    let settings = [(
        "git.toml",
        "untracked-max-bytes = 10\nuntracked-max-files = 1\n",
    )]
    .map(|(name, text)| (PathBuf::from(name), text.to_owned()));
    editor.set_settings(Settings::from_files(settings));
    editor.visit_file(&repository.0.join("lib.rs")).unwrap();
    editor.run_command("git-status");
    wait_until(&mut editor, "the untracked files", |editor| {
        editor
            .outline(0..100)
            .is_some_and(|outline| outline.rows.len() == 5)
    });

    let unfold = |editor: &mut Editor, row: &str, shown: &str| {
        go_to_row(editor, row);
        press(editor, "TAB");
        wait_until(editor, shown, |editor| {
            rows(editor).contains(&shown.to_owned())
        });
    };
    unfold(&mut editor, "? ./draft.md", "+one");
    assert!(rows(&editor).contains(&"@@ -0,0 +1 @@".to_owned()));
    unfold(&mut editor, "? ./big.txt", " Large file, 12 B, not shown");
    unfold(&mut editor, "? ./data.bin", " Binary file");
    unfold(&mut editor, "? ./notes/", "? notes/a.md");
    assert!(rows(&editor).contains(&"… 1 more files".to_owned()));
    unfold(&mut editor, "? notes/a.md", "+alpha");

    go_to_row(&mut editor, "+alpha");
    press(&mut editor, "s");
    wait_for_status(
        &mut editor,
        &repository,
        "A  notes/a.md\n?? big.txt\n?? data.bin\n?? draft.md\n?? notes/deep/b.md\n",
    );
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
fn every_diff_is_shown_or_hidden_at_once() {
    let repository = Repository::new("toggle-all");
    commit(&repository, "a.rs", "a\n", "Add a");
    commit(&repository, "b.rs", "b\n", "Add b");
    repository.write("a.rs", "a2\n");
    repository.write("b.rs", "b2\n");
    repository.git(&["add", "b.rs"]);
    let mut editor = status_buffer(&repository);
    wait_until(&mut editor, "both changes", |editor| {
        rows(editor).len() == 4
    });
    let hunks = |editor: &Editor| {
        rows(editor)
            .iter()
            .filter(|row| row.starts_with("@@"))
            .count()
    };

    press(&mut editor, "S-TAB");
    wait_until(&mut editor, "both diffs", |editor| hunks(editor) == 2);
    assert!(rows(&editor).contains(&"+a2".to_owned()));
    assert!(rows(&editor).contains(&"+b2".to_owned()));

    press(&mut editor, "S-TAB");
    assert_eq!(hunks(&editor), 0);
    assert_eq!(rows(&editor).len(), 4);
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

/// Each commit of the history in short: `[feature] Grow the feature`.
fn history(editor: &Editor) -> Vec<String> {
    editor
        .outline(0..100)
        .unwrap()
        .history
        .unwrap()
        .commits
        .into_iter()
        .map(|commit| {
            let chips: Vec<String> = commit.refs.into_iter().map(|chip| chip.name).collect();
            if chips.is_empty() {
                commit.subject
            } else {
                format!("[{}] {}", chips.join(" "), commit.subject)
            }
        })
        .collect()
}

#[test]
fn the_history_shows_all_branches_or_the_current_one_and_searches_messages() {
    let repository = Repository::new("history");
    let _remote = Remote::for_repository(&repository);
    repository.git(&["tag", "v1"]);
    repository.git(&["switch", "-q", "-c", "feature"]);
    commit(&repository, "feature.rs", "f\n", "Start the feature");
    commit(&repository, "feature.rs", "f\ng\n", "Grow the feature");
    repository.git(&["switch", "-q", "main"]);
    commit(&repository, "main.rs", "m\n", "Move main on");
    repository.git(&["switch", "-q", "feature"]);
    let mut editor = status_buffer(&repository);

    wait_until(&mut editor, "the history", |editor| {
        history(editor).len() == 4
    });
    let all = history(&editor);
    assert!(all.contains(&"[feature] Grow the feature".to_owned()));
    assert!(all.contains(&"[main] Move main on".to_owned()));
    assert_eq!(all.last().unwrap(), "[origin/main v1] First");
    let graph = editor.outline(0..100).unwrap().history.unwrap().commits;
    assert!(graph.iter().any(|commit| commit.graph.lane == 1));

    press(&mut editor, "l");
    wait_until(&mut editor, "the current branch's history", |editor| {
        history(editor).len() == 3
    });
    assert_eq!(
        history(&editor),
        [
            "[feature] Grow the feature",
            "Start the feature",
            "[origin/main v1] First"
        ]
    );

    press(&mut editor, "/");
    type_text(&mut editor, "grow");
    press(&mut editor, "RET");
    wait_until(&mut editor, "the search", |editor| {
        history(editor).len() == 1
    });
    assert_eq!(history(&editor), ["[feature] Grow the feature"]);
}

#[test]
fn a_commit_of_the_history_shows_its_message_files_and_diffs() {
    let repository = Repository::new("commit-view");
    commit(&repository, "lib.rs", "one\ntwo\n", "Grow lib");
    repository.write("src/new.rs", "fresh\n");
    repository.write("lib.rs", "one\ntwo\nthree\n");
    repository.git(&["add", "."]);
    repository.git(&[
        "commit",
        "-q",
        "-m",
        "Add new\n\nWhy it was added.\nAnd more.",
    ]);
    let mut editor = status_buffer(&repository);
    wait_until(&mut editor, "the history", |editor| {
        history(editor).len() == 3
    });

    press(&mut editor, "M-n");
    press(&mut editor, "M-n");
    let selected = |editor: &Editor| editor.outline(0..100).unwrap().history.unwrap().selected;
    assert_eq!(selected(&editor), Some(1));
    press(&mut editor, "M-p");
    press(&mut editor, "M-RET");
    wait_for_commit(&mut editor);
    let side = |editor: &Editor| editor.text_controls().unwrap().side.unwrap();
    let side_facts: Vec<String> = side(&editor).facts.into_iter().map(describe_fact).collect();
    assert_eq!(side_facts[0].split(" · ").nth(1), Some("Add new"));
    assert!(side_facts[2].starts_with("Parent: ") && side_facts[2].ends_with("Grow lib"));
    let side_rows = |editor: &Editor| -> Vec<String> {
        let side = editor.text_controls().unwrap().side.unwrap();
        side.rows.iter().cloned().map(describe_row).collect()
    };
    assert_eq!(
        side_rows(&editor),
        [
            "v Message 2",
            " Why it was added.",
            " And more.",
            "v Files 2",
            "M ./lib.rs +1 -0",
            "A src/new.rs +1 -0",
        ]
    );
    let text = editor.window_buffer().text().snapshot();
    assert_eq!(
        String::from_utf8_lossy(&text.bytes_in(0..text.len())),
        "M lib.rs  +1 −0\n@@ -1,2 +1,3 @@\n one\n two\n+three\n\
         A src/new.rs  +1 −0\n@@ -0,0 +1 @@\n+fresh"
    );
    let extras = editor.line_extras(0..8);
    let numbers: Vec<_> = extras
        .iter()
        .map(|extra| extra.number.as_deref().unwrap())
        .collect();
    assert_eq!(
        numbers,
        ["   ", "   ", "1 1", "2 2", "  3", "   ", "   ", "  1"]
    );
    let fills: Vec<_> = extras.iter().map(|extra| extra.fill).collect();
    assert_eq!(
        fills[..5],
        [Some(Tone::Faint), None, None, None, Some(Tone::Success)]
    );
    let plus = text.line_start(4).unwrap();
    assert!(
        editor
            .decorations(plus..plus + 1)
            .concat()
            .iter()
            .any(|span| span.face
                == Face::Console(ConsoleStyle {
                    color: Some(ConsoleColor::Indexed(2)),
                    ..ConsoleStyle::default()
                }))
    );

    // Point's file is the one selected beside the diff.
    assert_eq!(side(&editor).selected, Some(4));
    let moves = [
        ("n", 1),
        ("n", 6),
        ("p", 1),
        ("N", 5),
        ("M-p", 0),
        ("M-n", 5),
    ];
    for (keys, line) in moves {
        press(&mut editor, keys);
        assert_eq!(editor.window_buffer().point_line(), line, "after {keys}");
    }
    assert_eq!(side(&editor).selected, Some(5));
    let click = side(&editor).actions[4].clone().unwrap();
    editor.run_action(&click);
    assert_eq!(editor.window_buffer().point_line(), 0);
    press(&mut editor, "TAB");
    assert_eq!(side_rows(&editor)[..2], ["> Message 2", "v Files 2"]);

    editor.set_point_at(4, 0);
    press(&mut editor, "o");
    let name = "lib.rs @ ".to_owned() + &short_head(&repository);
    wait_until(&mut editor, "the file at the commit", |editor| {
        editor.window_buffer().name() == name
    });
    let text = editor.window_buffer().text().snapshot();
    assert_eq!(text.bytes_in(0..text.len()), b"one\ntwo\nthree\n");
    assert_eq!(editor.window_buffer().point_line(), 2);

    editor.switch_to_buffer(&format!("Commit {}", short_head(&repository)));
    press(&mut editor, "RET");
    assert_eq!(
        editor.window_buffer().path(),
        Some(repository.0.join("lib.rs").as_path())
    );
    assert_eq!(editor.window_buffer().point_line(), 2);
}

#[test]
fn the_log_of_a_file_shows_its_commits_and_opens_them_at_the_file() {
    let repository = Repository::new("file-log");
    commit(&repository, "lib.rs", "one\ntwo\n", "Grow lib");
    commit(&repository, "other.rs", "x\n", "Add other");
    let mut editor = editor();
    editor.visit_file(&repository.0.join("lib.rs")).unwrap();
    press(&mut editor, "C-x v l");
    assert_eq!(editor.window_buffer().mode(), "Git");
    wait_until(&mut editor, "the file's history", |editor| {
        editor
            .outline(0..100)
            .and_then(|outline| outline.history)
            .is_some_and(|history| history.commits.len() == 2)
    });
    assert_eq!(history(&editor), ["Grow lib", "First"]);
    let log = editor.outline(0..100).unwrap().history.unwrap();
    assert_eq!(log.file.as_ref().unwrap().label, "lib.rs");

    editor.run_action(&log.commits[0].open);
    wait_for_commit(&mut editor);
    let side = editor.text_controls().unwrap().side.unwrap();
    let side_rows: Vec<_> = side.rows.iter().cloned().map(describe_row).collect();
    assert_eq!(side_rows, ["v Files 1", "M ./lib.rs +1 -0"]);
    assert_eq!(editor.window_buffer().point_line(), 0);
    let text = editor.window_buffer().text().snapshot();
    assert!(String::from_utf8_lossy(&text.bytes_in(0..text.len())).contains("\n+two"));

    press(&mut editor, "q");
    let clear = editor
        .outline(0..100)
        .unwrap()
        .history
        .unwrap()
        .file
        .unwrap()
        .action;
    editor.run_action(&clear);
    wait_until(&mut editor, "every file's history", |editor| {
        history(editor).len() == 3
    });
}

fn wait_for_commit(editor: &mut Editor) {
    wait_until(editor, "the commit", |editor| {
        editor.window_buffer().mode() == "Git Commit"
    });
}

fn short_head(repository: &Repository) -> String {
    repository
        .git(&["rev-parse", "--short=7", "HEAD"])
        .trim()
        .to_owned()
}

fn candidates(editor: &Editor) -> Vec<String> {
    editor
        .minibuffer()
        .unwrap()
        .candidates()
        .iter()
        .map(|candidate| {
            let verb = candidate
                .verb
                .map_or(String::new(), |verb| format!("{verb} "));
            let tag = candidate
                .tag
                .map_or(String::new(), |tag| format!(" [{tag}]"));
            format!("{verb}{}{tag} · {}", candidate.label, candidate.annotation)
        })
        .collect()
}

#[test]
fn branches_are_switched_to_or_created_from_a_base_they_remember() {
    let repository = Repository::new("branch");
    let _remote = Remote::for_repository(&repository);
    repository.git(&["switch", "-q", "-c", "other"]);
    commit(&repository, "other.rs", "o\n", "Other work");
    repository.git(&["push", "-q", "origin", "other"]);
    repository.git(&["switch", "-q", "main"]);
    repository.git(&["branch", "-q", "-D", "other"]);
    let main_id = repository.git(&["rev-parse", "--short", "main"]);
    let mut editor = status_buffer(&repository);

    press(&mut editor, "b");
    assert_eq!(
        candidates(&editor),
        [
            "main [current] · ",
            "origin/main · remote",
            "origin/other · remote"
        ]
    );
    type_text(&mut editor, "topic");
    assert_eq!(
        candidates(&editor),
        [format!("Create topic · from main · {}", main_id.trim())]
    );
    press(&mut editor, "TAB");
    assert!(candidates(&editor)[0].starts_with("Create topic · from origin/main · "));
    press(&mut editor, "RET");
    wait_until(&mut editor, "the new branch", |_| {
        repository.git(&["branch", "--show-current"]).trim() == "topic"
    });
    assert_eq!(
        repository
            .git(&["config", "branch.topic.remacs-base"])
            .trim(),
        "origin/main"
    );
    wait_until(&mut editor, "its base", |editor| {
        facts(editor).contains(&"Base: origin/main · Up to date".to_owned())
    });

    press(&mut editor, "b");
    type_text(&mut editor, "other");
    assert_eq!(candidates(&editor), ["origin/other · remote"]);
    press(&mut editor, "RET");
    wait_until(&mut editor, "the tracking branch", |_| {
        repository.git(&["branch", "--show-current"]).trim() == "other"
    });
    assert_eq!(
        repository
            .git(&["rev-parse", "--abbrev-ref", "other@{upstream}"])
            .trim(),
        "origin/other"
    );
}

/// Waits for the last commit's message to be `expected`.
fn wait_for_message(editor: &mut Editor, repository: &Repository, expected: &str) {
    let start = Instant::now();
    loop {
        let message = repository.git(&["log", "-1", "--format=%B"]);
        if message.trim_end() == expected {
            return;
        }
        assert!(start.elapsed() < TIMEOUT, "the message stayed {message:?}");
        thread::sleep(Duration::from_millis(10));
        editor.refresh();
    }
}

#[test]
fn a_commit_message_has_a_subject_and_a_description() {
    let repository = Repository::new("commit");
    repository.write("lib.rs", "two\n");
    repository.git(&["add", "lib.rs"]);
    let mut editor = status_buffer(&repository);

    press(&mut editor, "c");
    let prompt = editor.minibuffer().unwrap().prompt();
    assert_eq!(prompt.items_summary, "1 file · +1 −1");
    assert_eq!(prompt.subject, "Test → `main`");
    type_text(&mut editor, "Say two");
    press(&mut editor, "RET");
    assert!(editor.minibuffer().unwrap().in_description());
    type_text(&mut editor, "One was");
    press(&mut editor, "RET");
    type_text(&mut editor, "not enough.");
    press(&mut editor, "M-s");
    press(&mut editor, "C-c C-c");
    assert!(editor.minibuffer().is_none());
    wait_for_message(
        &mut editor,
        &repository,
        "Say two\n\nOne was\nnot enough.\n\nSigned-off-by: Test <test@example.com>",
    );

    repository.write("kept.rs", "changed\n");
    repository.git(&["add", "kept.rs"]);
    wait_until(&mut editor, "the staged change", |editor| {
        rows(editor).contains(&"M ./kept.rs +1 -1".to_owned())
    });
    press(&mut editor, "c");
    press(&mut editor, "M-p");
    assert_eq!(editor.minibuffer().unwrap().input_text(), "Say two");
    press(&mut editor, "M-n");
    assert_eq!(editor.echo().as_deref(), Some("No later message"));
    let subject_length = "Say two".len();
    for _ in 0..subject_length {
        press(&mut editor, "DEL");
    }
    press(&mut editor, "TAB M-< C-SPC M-> C-w");
    press(&mut editor, "C-c C-c");
    assert_eq!(
        editor.echo().as_deref(),
        Some("The commit message is empty")
    );
    assert!(editor.minibuffer().is_some());

    press(&mut editor, "M-a");
    press(&mut editor, "C-c C-c");
    wait_for_status(&mut editor, &repository, "");
    assert_eq!(repository.git(&["rev-list", "--count", "HEAD"]).trim(), "2");
}

/// A branch with commits A, B and C on `main`, which then moved on.
fn diverged(name: &str) -> Repository {
    let repository = Repository::new(name);
    repository.git(&["switch", "-q", "-c", "feature"]);
    for name in ["A", "B", "C"] {
        commit(&repository, &format!("{name}.rs"), name, name);
    }
    repository.git(&["switch", "-q", "main"]);
    commit(&repository, "main.rs", "m\n", "Move main on");
    repository.git(&["switch", "-q", "feature"]);
    repository
}

/// The steps in short: `pick A`.
fn steps(editor: &Editor) -> Vec<String> {
    editor
        .minibuffer()
        .unwrap()
        .candidates()
        .iter()
        .map(|candidate| {
            let (action, _) = candidate.action.unwrap();
            format!("{action} {}", candidate.annotation)
        })
        .collect()
}

#[test]
fn a_rebase_replays_the_commits_as_chosen() {
    let repository = diverged("rebase");
    let mut editor = status_buffer(&repository);
    wait_until(&mut editor, "the base", |editor| {
        facts(editor).contains(&"Base: main · 1 new commit".to_owned())
    });

    press(&mut editor, "r");
    assert_eq!(steps(&editor), ["pick A", "pick B", "pick C"]);
    let minibuffer = editor.minibuffer().unwrap();
    assert_eq!(minibuffer.input_text(), "main");
    assert_eq!(minibuffer.subject(), "`feature` onto `main`");
    let (summary, _) = minibuffer.completions().outcome.clone().unwrap();
    assert_eq!(summary, "3 commits on top of main");

    press(&mut editor, "s");
    press(&mut editor, "RET");
    assert_eq!(
        editor.echo().as_deref(),
        Some(&*format!(
            "{} has nothing before it to fold into",
            repository
                .git(&["rev-parse", "--short", "feature~2"])
                .trim()
        ))
    );
    assert!(editor.minibuffer().is_some());

    press(&mut editor, "d C-n C-n s C-p M-<down>");
    assert_eq!(steps(&editor), ["drop A", "squash C", "pick B"]);
    press(&mut editor, "M-<up>");
    assert_eq!(steps(&editor), ["drop A", "pick B", "squash C"]);
    press(&mut editor, "RET");
    wait_until(&mut editor, "the rebase", |_| {
        repository.git(&["log", "--format=%s", "main..feature"]) == "B\n"
    });
    assert_eq!(
        repository.git(&["log", "-1", "--format=%B"]).trim(),
        "B\n\nC"
    );
    assert!(!repository.0.join("A.rs").exists());
    assert!(repository.0.join("main.rs").exists());
}

#[test]
fn a_merge_brings_the_base_in() {
    let repository = diverged("merge");
    let mut editor = status_buffer(&repository);
    wait_until(&mut editor, "the base", |editor| {
        facts(editor).contains(&"Base: main · 1 new commit".to_owned())
    });

    press(&mut editor, "m");
    let minibuffer = editor.minibuffer().unwrap();
    assert_eq!(minibuffer.label(), "Merge");
    assert_eq!(minibuffer.subject(), "`main` into `feature`");
    assert_eq!(minibuffer.candidates()[0].annotation, "Move main on");
    press(&mut editor, "RET");
    wait_until(&mut editor, "the merge", |_| {
        repository
            .git(&["log", "-1", "--format=%P"])
            .split_whitespace()
            .count()
            == 2
    });
    wait_until(&mut editor, "the base caught up", |editor| {
        facts(editor).contains(&"Base: main · Up to date".to_owned())
    });
}

fn toolbar_counts(editor: &Editor) -> Vec<(String, Option<usize>)> {
    editor
        .outline(0..100)
        .unwrap()
        .toolbar
        .into_iter()
        .map(|button| (button.label, button.count))
        .collect()
}

#[test]
fn the_branch_is_pushed_to_its_upstream_or_gets_one() {
    let repository = Repository::new("push");
    let _remote = Remote::for_repository(&repository);
    commit(&repository, "work.rs", "w\n", "Local work");
    let mut editor = status_buffer(&repository);
    wait_until(&mut editor, "the commit to push", |editor| {
        toolbar_counts(editor) == [("Pull".to_owned(), None), ("Push".to_owned(), Some(1))]
    });

    press(&mut editor, "P");
    let prompt = editor.minibuffer().unwrap().prompt();
    assert_eq!(
        prompt.title,
        Some(("Push `main`".to_owned(), "1 commit".to_owned()))
    );
    assert_eq!(prompt.items.len(), 1);
    press(&mut editor, "u");
    assert!(editor.minibuffer().is_none());
    wait_until(&mut editor, "the push", |_| {
        repository.git(&["rev-parse", "origin/main"]) == repository.git(&["rev-parse", "main"])
    });

    repository.git(&["switch", "-q", "-c", "topic"]);
    commit(&repository, "topic.rs", "t\n", "Topic work");
    wait_until(&mut editor, "the new branch", |editor| {
        facts(editor)[0].starts_with("Head: topic")
    });
    press(&mut editor, "P");
    let first = &editor.minibuffer().unwrap().candidates()[0];
    assert_eq!(
        (first.label.as_str(), first.annotation.as_str()),
        ("origin/topic", "new upstream")
    );
    press(&mut editor, "RET");
    wait_until(&mut editor, "the new upstream", |_| {
        output(git(&repository.0).args(["rev-parse", "--abbrev-ref", "topic@{upstream}"]))
            .is_ok_and(|printed| printed == b"origin/topic\n")
    });
}

#[test]
fn a_diverged_branch_is_pulled_by_rebase_or_merge() {
    let repository = Repository::new("pull");
    let _remote = Remote::for_repository(&repository);
    commit(&repository, "incoming.rs", "i\n", "Incoming");
    repository.git(&["push", "-q"]);
    repository.git(&["reset", "-q", "--hard", "HEAD~1"]);
    commit(&repository, "mine.rs", "m\n", "Mine");
    let mut editor = status_buffer(&repository);
    wait_until(&mut editor, "the divergence", |editor| {
        toolbar_counts(editor) == [("Pull".to_owned(), Some(1)), ("Push".to_owned(), Some(1))]
    });

    press(&mut editor, "F");
    let note = |editor: &Editor| {
        let (_, chain) = editor
            .minibuffer()
            .unwrap()
            .completions()
            .outcome
            .clone()
            .unwrap();
        chain.note.unwrap()
    };
    assert_eq!(
        note(&editor),
        "Your commit is replayed on top of the incoming one. History stays linear."
    );
    press(&mut editor, "m");
    assert_eq!(
        note(&editor),
        "A merge commit joins your 1 commit and the incoming one."
    );
    press(&mut editor, "TAB RET");
    assert_eq!(
        editor.echo().as_deref(),
        Some("Fast-forward isn't possible: the branches have diverged")
    );
    press(&mut editor, "r RET");
    wait_until(&mut editor, "the pull", |_| {
        repository.git(&["log", "--format=%s", "-2"]) == "Mine\nIncoming\n"
    });
}

/// `lib.rs` changed on `feature` and on `main`, `feature` with one more commit after.
fn conflicting(name: &str) -> Repository {
    let repository = Repository::new(name);
    repository.git(&["switch", "-q", "-c", "feature"]);
    commit(&repository, "lib.rs", "feature\n", "Change on feature");
    commit(&repository, "second.rs", "s\n", "Second");
    repository.git(&["switch", "-q", "main"]);
    commit(&repository, "lib.rs", "main\n", "Change on main");
    repository.git(&["switch", "-q", "feature"]);
    repository
}

fn banner_state(editor: &Editor) -> String {
    editor.outline(0..100).unwrap().banner.unwrap().state.0
}

#[test]
fn a_rebase_stopped_at_a_conflict_is_resolved_and_continued() {
    let repository = conflicting("rebase-conflict");
    assert!(output(git(&repository.0).args(["rebase", "main"])).is_err());
    let mut editor = status_buffer(&repository);
    wait_until(&mut editor, "the conflict", |editor| {
        rows(editor).contains(&"U ./lib.rs".to_owned())
    });
    let facts = facts(&editor);
    assert_eq!(facts[0], "Head: feature · rebase in progress");
    assert!(facts[1].starts_with("Onto: main · ") && facts[1].ends_with(" Change on main"));
    assert!(facts[2].ends_with(" · Change on feature"));
    assert_eq!(banner_state(&editor), "1 conflict left");

    go_to_row(&mut editor, "U ./lib.rs");
    let conflict = editor.outline(0..100).unwrap().conflict.unwrap();
    assert_eq!((conflict.current, conflict.left), (1, 1));
    let block = conflict
        .regions
        .iter()
        .find_map(|region| match region {
            remacs_core::ConflictRegion::Conflict(block) => Some(block),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        (block.ours.role.as_str(), &block.ours.lines[..]),
        ("upstream", &["main".to_owned()][..])
    );
    assert_eq!(block.theirs.lines, ["feature"]);

    press(&mut editor, "C-c C-c");
    assert_eq!(editor.echo().as_deref(), Some("1 conflict left"));
    press(&mut editor, "t");
    assert_eq!(
        fs::read_to_string(repository.0.join("lib.rs")).unwrap(),
        "feature\n"
    );
    wait_until(&mut editor, "the resolution", |editor| {
        rows(editor).contains(&"✓ ./lib.rs +1 -1".to_owned())
    });
    assert_eq!(banner_state(&editor), "All conflicts resolved");

    press(&mut editor, "C-c C-c");
    let minibuffer = editor.minibuffer().unwrap();
    assert_eq!(minibuffer.input_text(), "Change on feature");
    assert_eq!(minibuffer.candidates()[0].annotation, "Second");
    press(&mut editor, "C-c C-c");
    wait_until(&mut editor, "the rebase", |_| {
        repository.git(&["log", "--format=%s", "main..feature"]) == "Second\nChange on feature\n"
    });
    wait_until(&mut editor, "the banner gone", |editor| {
        editor.outline(0..100).unwrap().banner.is_none()
    });
}

#[test]
fn a_merge_conflict_keeps_both_sides_or_is_aborted() {
    let repository = conflicting("merge-conflict");
    assert!(output(git(&repository.0).args(["merge", "main"])).is_err());
    let mut editor = status_buffer(&repository);
    wait_until(&mut editor, "the conflict", |editor| {
        rows(editor).contains(&"U ./lib.rs".to_owned())
    });
    assert_eq!(facts(&editor)[0], "Head: feature · merge in progress");
    go_to_row(&mut editor, "U ./lib.rs");
    press(&mut editor, "b");
    assert_eq!(
        fs::read_to_string(repository.0.join("lib.rs")).unwrap(),
        "feature\nmain\n"
    );

    press(&mut editor, "C-c C-k");
    press(&mut editor, "y");
    wait_until(&mut editor, "the merge aborted", |editor| {
        editor.outline(0..100).unwrap().banner.is_none()
    });
    assert_eq!(
        fs::read_to_string(repository.0.join("lib.rs")).unwrap(),
        "feature\n"
    );
}

/// The stash prompt's file count and what the changes are.
fn stash_note(editor: &Editor) -> (String, String) {
    let minibuffer = editor.minibuffer().unwrap();
    (
        minibuffer.note().unwrap_or_default().to_owned(),
        minibuffer.subject().to_owned(),
    )
}

/// The selected stash's files, as `M lib.rs`.
fn stash_files(editor: &Editor) -> Vec<String> {
    let candidate = editor.minibuffer().unwrap().selected_candidate().unwrap();
    candidate
        .expansion
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|item| format!("{} {}", item.state.unwrap().letter(), item.label))
        .collect()
}

fn stash_list(repository: &Repository) -> String {
    repository.git(&["stash", "list", "--format=%gs"])
}

/// Waits for a job to say it is done.
fn wait_for_echo(editor: &mut Editor, expected: &str) {
    wait_until(editor, expected, |editor| {
        editor.echo().as_deref() == Some(expected)
    });
}

#[test]
fn changes_are_stashed_with_a_message_and_applied_or_popped() {
    let repository = Repository::new("stash");
    repository.write("lib.rs", "two\n");
    repository.write("new.md", "idea\n");
    let mut editor = status_buffer(&repository);
    wait_until(&mut editor, "the changes", |editor| rows(editor).len() == 4);

    press(&mut editor, "z");
    assert_eq!(stash_note(&editor), ("1 file".into(), "1 unstaged".into()));
    assert!(editor.minibuffer().unwrap().candidates().is_empty());
    press(&mut editor, "M-u");
    assert_eq!(
        stash_note(&editor),
        ("2 files".into(), "1 unstaged · 1 untracked".into())
    );
    type_text(&mut editor, "parked work");
    press(&mut editor, "RET");
    wait_for_echo(&mut editor, "Stashed the changes");
    assert_eq!(repository.short_status(), "");
    assert_eq!(stash_list(&repository), "On main: parked work\n");

    wait_until(&mut editor, "the stash", |editor| {
        rows(editor) == ["> Stashes 1"]
    });
    press(&mut editor, "z");
    assert_eq!(candidates(&editor), ["stash@{0} · On main: parked work"]);
    assert_eq!(stash_files(&editor), ["M lib.rs", "? new.md"]);
    press(&mut editor, "RET");
    assert_eq!(editor.echo().as_deref(), Some("No changes to stash"));
    press(&mut editor, "TAB");
    assert!(editor.minibuffer().unwrap().in_list());
    press(&mut editor, "RET");
    assert!(editor.minibuffer().is_none());
    assert_eq!(
        rows(&editor),
        [
            "v Stashes 1",
            "stash@{0} On main: parked work",
            "M ./lib.rs +1 -1",
            "@@ -1 +1 @@",
            "-one",
            "+two",
            "? ./new.md +1 -0",
            "@@ -0,0 +1 @@",
            "+idea",
        ]
    );
    press(&mut editor, "TAB");
    assert_eq!(
        rows(&editor),
        ["v Stashes 1", "stash@{0} On main: parked work"]
    );

    press(&mut editor, "z");
    press(&mut editor, "TAB");
    press(&mut editor, "a");
    wait_for_echo(&mut editor, "Applied stash@{0}");
    assert_eq!(repository.short_status(), " M lib.rs\n?? new.md\n");
    assert_eq!(stash_list(&repository), "On main: parked work\n");

    repository.git(&["checkout", "-q", "--", "."]);
    repository.git(&["clean", "-q", "-f"]);
    wait_for_status(&mut editor, &repository, "");
    press(&mut editor, "z");
    press(&mut editor, "TAB");
    press(&mut editor, "p");
    wait_for_echo(&mut editor, "Popped stash@{0}");
    assert_eq!(repository.short_status(), " M lib.rs\n?? new.md\n");
    assert_eq!(stash_list(&repository), "");
}

#[test]
fn a_stash_is_dropped_after_asking_or_becomes_a_branch() {
    let repository = Repository::new("stash-drop");
    repository.write("lib.rs", "two\n");
    repository.git(&["stash", "push", "-q", "-m", "first"]);
    repository.write("kept.rs", "changed\n");
    repository.git(&["stash", "push", "-q", "-m", "second"]);
    let mut editor = status_buffer(&repository);
    wait_until(&mut editor, "the stashes", |editor| {
        rows(editor) == ["> Stashes 2"]
    });
    press(&mut editor, "TAB");
    go_to_row(&mut editor, "stash@{0} On main: second");
    press(&mut editor, "k");
    assert_eq!(
        editor.minibuffer().unwrap().dialog().unwrap().title,
        "Drop `stash@{0}`?"
    );
    press(&mut editor, "y");
    wait_for_echo(&mut editor, "Dropped stash@{0}");
    assert_eq!(stash_list(&repository), "On main: first\n");

    press(&mut editor, "z");
    assert_eq!(stash_files(&editor), ["M lib.rs"]);
    press(&mut editor, "TAB");
    press(&mut editor, "b");
    type_text(&mut editor, "rescued");
    press(&mut editor, "RET");
    wait_for_echo(&mut editor, "Switched to rescued, with stash@{0} applied");
    assert_eq!(repository.short_status(), " M lib.rs\n");
    assert_eq!(repository.git(&["branch", "--show-current"]), "rescued\n");
    assert_eq!(stash_list(&repository), "");
}

#[test]
fn open_buffers_follow_discards_and_branch_switches_without_auto_revert() {
    let repository = Repository::new("follow");
    repository.git(&["switch", "-q", "-c", "other"]);
    commit(&repository, "kept.rs", "kept on other\n", "Other");
    repository.git(&["switch", "-q", "main"]);
    repository.write("lib.rs", "changed\n");
    let mut editor = editor();
    let settings = [(
        PathBuf::from("files.toml"),
        "auto-revert = false\n".to_owned(),
    )];
    editor.set_settings(Settings::from_files(settings));
    editor.visit_file(&repository.0.join("kept.rs")).unwrap();
    editor.visit_file(&repository.0.join("lib.rs")).unwrap();
    editor.run_command("git-status");
    wait_until(&mut editor, "the change", |editor| {
        rows(editor).contains(&"M ./lib.rs +1 -1".to_owned())
    });
    let contents = |editor: &Editor, name: &str| {
        let buffer = editor.buffers().iter().find(|buffer| buffer.name() == name);
        let buffer = buffer.unwrap();
        buffer.contents(0..buffer.text().len())
    };

    go_to_row(&mut editor, "M ./lib.rs +1 -1");
    press(&mut editor, "k y");
    wait_until(&mut editor, "lib.rs read again", |editor| {
        contents(editor, "lib.rs") == "one\n"
    });

    press(&mut editor, "b");
    type_text(&mut editor, "other");
    press(&mut editor, "RET");
    wait_until(&mut editor, "kept.rs read again", |editor| {
        contents(editor, "kept.rs") == "kept on other\n"
    });
}
