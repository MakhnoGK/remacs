//! Setting changes aside and bringing them back, like Emacs `magit-stash`: a prompt that
//! stashes with a message and lists the stashes to apply, pop, branch from or drop.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use remacs_core::{
    Candidate, CommandResult, Completions, Dialog, Editor, EntryAction, Expansion, FileState,
    Keymap, LineCounts, ListItem, Prompt, PromptOption,
};

use crate::command::{git, output};
use crate::commands::{run_saying, shown_status};
use crate::diff::{FileDiff, split_patch};
use crate::history_view::ago;
use crate::monitor::Job;
use crate::status::{Head, Snapshot};
use crate::status_view::{self, Line, NOT_A_STATUS_BUFFER, StatusView, directory_label};

pub const APPLY: &str = "git-stash-apply";
pub const POP: &str = "git-stash-pop";
pub const BRANCH: &str = "git-stash-branch";
pub const SHOW: &str = "git-stash-show";
pub const DROP: &str = "git-stash-drop";
const NEXT_FIELD: &str = "minibuffer-next-field";
const ACTIONS: [EntryAction; 5] = [
    action("Apply", "a", APPLY),
    action("Pop", "p", POP),
    action("New branch", "b", BRANCH),
    action("Show diff", "↵", SHOW),
    EntryAction {
        destructive: true,
        ..action("Drop", "k", DROP)
    },
];
const LIST_KEYS: &[(&str, &str)] = &[
    ("a", APPLY),
    ("p", POP),
    ("b", BRANCH),
    ("RET", SHOW),
    ("k", DROP),
];
const HINTS: &[(&str, &str)] = &[
    ("↵", "Stash changes"),
    ("⇥", "Go to list"),
    ("a p k", "Apply · pop · drop"),
    ("C-g", "Cancel"),
];
const LIST_HINTS: &[(&str, &str)] = &[
    ("↵", "Show diff"),
    ("⇥", "Back to message"),
    ("a p b k", "Apply · pop · branch · drop"),
    ("C-g", "Cancel"),
];
const GROUP: &str = "Stashes";
/// Its format: a record per stash, which `-z` ends its fields and files with.
const RECORD: &str = "--format=%x01%gd%x00%H%x00%ct%x00%P%x00%s";
/// Of a stash's commit's parents, the one holding its untracked files.
const UNTRACKED_PARENT: usize = 2;

const fn action(label: &'static str, key: &'static str, command: &'static str) -> EntryAction {
    EntryAction {
        label,
        key,
        command,
        destructive: false,
    }
}

/// A stash and the files it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashEntry {
    /// `stash@{0}`, or empty when read by id.
    pub name: String,
    pub id: String,
    pub message: String,
    pub time: i64,
    pub files: Vec<StashFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashFile {
    pub path: PathBuf,
    pub state: FileState,
    pub lines: Option<LineCounts>,
}

/// A stash shown unfolded in the status buffer: its files and their diffs.
pub struct StashContents {
    pub files: Vec<StashFile>,
    pub diffs: HashMap<PathBuf, FileDiff>,
}

/// Reads a message and stashes the changes with it; lists the stashes to act on.
pub fn stash(editor: &mut Editor) -> CommandResult {
    let (root, snapshot) = shown_status(editor)?;
    if snapshot.operation.is_some() {
        return Err("Finish the rebase or merge first".into());
    }
    if matches!(snapshot.head, Head::Unborn(_)) {
        return Err("Commit something first".into());
    }
    let changes = Changes::of(&snapshot);
    let stashes = list(&root)?;
    if changes.is_empty() && stashes.is_empty() {
        return Err("No changes to stash, and no stashes".into());
    }
    let [keep_staged, untracked, only_staged] =
        [true, false, false].map(|on| Rc::new(Cell::new(on)));
    let hidden_while_only_staged = |option: PromptOption| PromptOption {
        hidden_while: Some(only_staged.clone()),
        ..option
    };
    let mut keymap = Keymap::default();
    keymap.bind("TAB", NEXT_FIELD);
    let mut list_keymap = Keymap::default();
    for &(keys, command) in LIST_KEYS {
        list_keymap.bind(keys, command);
    }
    let check = {
        let (untracked, only_staged) = (untracked.clone(), only_staged.clone());
        move |_: &str| match changes.to_stash(untracked.get(), only_staged.get()) {
            0 if only_staged.get() => Err("Nothing staged to stash".to_owned()),
            0 if changes.untracked > 0 => {
                Err("Only untracked files: include them with M-u".to_owned())
            }
            0 => Err("No changes to stash".to_owned()),
            _ => Ok(()),
        }
    };
    let prompt = Prompt {
        label: "Stash".into(),
        placeholder: "What the changes are (optional)",
        wide: true,
        hints: HINTS,
        list_hints: LIST_HINTS,
        options: vec![
            hidden_while_only_staged(PromptOption::chip('k', "Keep staged changes", &keep_staged)),
            hidden_while_only_staged(PromptOption::chip('u', "Include untracked", &untracked)),
            PromptOption::chip('s', "Only staged", &only_staged),
        ],
        chips_first: true,
        keymap: Some(keymap),
        list_keymap: Some(list_keymap),
        check: Some(Rc::new(check)),
        ..Prompt::default()
    };
    let candidates: Vec<Candidate> = stashes.iter().map(stash_candidate).collect();
    let complete = {
        let (untracked, only_staged) = (untracked.clone(), only_staged.clone());
        move |_: &Editor, _: &str| Completions {
            total: candidates.len(),
            candidates: candidates.clone(),
            note: Some(plural(
                changes.to_stash(untracked.get(), only_staged.get()),
                "file",
            )),
            subject: Some(changes.summary(untracked.get() && !only_staged.get())),
            count_groups: true,
            ..Completions::default()
        }
    };
    editor.read(prompt, complete, move |editor, message| {
        let mut args = vec!["stash".to_owned(), "push".to_owned()];
        if only_staged.get() {
            args.push("--staged".into());
        } else {
            if keep_staged.get() {
                args.push("--keep-index".into());
            }
            if untracked.get() {
                args.push("--include-untracked".into());
            }
        }
        let message = message.trim();
        if !message.is_empty() {
            args.extend(["--message".to_owned(), message.to_owned()]);
        }
        run_saying(
            editor,
            &root,
            git_job(args, "git stash"),
            "Stashing…".into(),
            "Stashed the changes".into(),
        )
    })
}

/// How many files have changes of each kind.
#[derive(Clone, Copy)]
struct Changes {
    staged: usize,
    unstaged: usize,
    /// Tracked files with changes, staged or not.
    tracked: usize,
    untracked: usize,
}

impl Changes {
    fn of(snapshot: &Snapshot) -> Self {
        let entries = || snapshot.files.values().filter(|entry| !entry.conflicted);
        let tracked = || entries().filter(|entry| entry.staged != b'?');
        Self {
            staged: tracked().filter(|entry| entry.staged != b'.').count(),
            unstaged: tracked().filter(|entry| entry.unstaged != b'.').count(),
            tracked: tracked().count(),
            untracked: entries().filter(|entry| entry.staged == b'?').count()
                + snapshot.untracked_directories.len(),
        }
    }

    fn is_empty(self) -> bool {
        self.tracked == 0 && self.untracked == 0
    }

    /// Files a stash takes with these settings.
    fn to_stash(self, untracked: bool, only_staged: bool) -> usize {
        match () {
            _ if only_staged => self.staged,
            _ if untracked => self.tracked + self.untracked,
            _ => self.tracked,
        }
    }

    /// `2 staged · 2 unstaged`.
    fn summary(self, with_untracked: bool) -> String {
        let untracked = if with_untracked { self.untracked } else { 0 };
        [
            (self.staged, "staged"),
            (self.unstaged, "unstaged"),
            (untracked, "untracked"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, kind)| format!("{count} {kind}"))
        .collect::<Vec<_>>()
        .join(" · ")
    }
}

fn stash_candidate(entry: &StashEntry) -> Candidate {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    let when = ago(now - entry.time);
    Candidate {
        annotation: entry.message.clone(),
        detail: Some(format!("{} · {when}", plural(entry.files.len(), "file")).into()),
        group: Some(GROUP.into()),
        expansion: Some(Rc::new(Expansion {
            items: entry.files.iter().map(file_item).collect(),
            actions: ACTIONS.to_vec(),
            detail: when,
        })),
        ..Candidate::new(entry.name.clone())
    }
}

fn file_item(file: &StashFile) -> ListItem {
    ListItem {
        icon: None,
        mark: None,
        label: file
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        detail: directory_label(&file.path),
        state: Some(file.state),
        lines: file.lines,
    }
}

/// Applies the stash, keeping it, like `git stash apply`.
pub fn apply(editor: &mut Editor) -> CommandResult {
    let (root, name) = target(editor)?;
    let job = git_job(
        vec!["stash".into(), "apply".into(), name.clone()],
        "git stash",
    );
    run_saying(
        editor,
        &root,
        job,
        format!("Applying {name}…"),
        format!("Applied {name}"),
    )
}

/// Applies the stash and drops it, unless it conflicts, like `git stash pop`.
pub fn pop(editor: &mut Editor) -> CommandResult {
    let (root, name) = target(editor)?;
    let job = git_job(
        vec!["stash".into(), "pop".into(), name.clone()],
        "git stash",
    );
    run_saying(
        editor,
        &root,
        job,
        format!("Popping {name}…"),
        format!("Popped {name}"),
    )
}

/// Reads a name, then makes a branch of it where the stash was made, switches to it and
/// applies the stash there, like `git stash branch`.
pub fn branch(editor: &mut Editor) -> CommandResult {
    let (root, name) = target(editor)?;
    let prompt = Prompt {
        label: "Branch".into(),
        placeholder: "Name of the new branch",
        hints: &[("↵", "Create and apply"), ("C-g", "Cancel")],
        check: Some(Rc::new(|branch: &str| {
            if branch.trim().is_empty() {
                Err("Name the branch".to_owned())
            } else {
                Ok(())
            }
        })),
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        move |editor, branch| {
            let branch = branch.trim().to_owned();
            let args = vec![
                "stash".into(),
                "branch".into(),
                branch.clone(),
                name.clone(),
            ];
            run_saying(
                editor,
                &root,
                git_job(args, "git stash"),
                format!("Making {branch} from {name}…"),
                format!("Switched to {branch}, with {name} applied"),
            )
        },
    )
}

/// Drops the stash after asking, like `git stash drop`.
pub fn drop_stash(editor: &mut Editor) -> CommandResult {
    let (root, name) = target(editor)?;
    let items: Vec<ListItem> = read(&root, &["-g", "refs/stash"])
        .into_iter()
        .find(|entry| entry.name == name)
        .map(|entry| entry.files.iter().map(file_item).collect())
        .unwrap_or_default();
    let title = format!("Drop {name}?");
    editor.ask_dialog(
        false,
        move |_| Dialog {
            title: title.clone(),
            explanation: "Its changes are gone for good.".into(),
            items: items.clone(),
            option: None,
            yes_label: "Drop".into(),
            no_label: "Cancel",
            destructive: true,
        },
        move |editor, yes, _| {
            if !yes {
                return Ok(());
            }
            let job = git_job(
                vec!["stash".into(), "drop".into(), name.clone()],
                "git stash",
            );
            run_saying(
                editor,
                &root,
                job,
                format!("Dropping {name}…"),
                format!("Dropped {name}"),
            )
        },
    )
}

/// Shows the stash's files and their diffs unfolded in the status buffer.
pub fn show(editor: &mut Editor) -> CommandResult {
    let (root, name) = target(editor)?;
    let (_, snapshot) = shown_status(editor)?;
    let index = snapshot
        .details
        .as_ref()
        .and_then(|details| details.stashes.iter().position(|stash| stash.name == name))
        .ok_or("The stash is gone")?;
    let id = snapshot
        .details
        .as_ref()
        .map(|details| details.stashes[index].id.clone());
    let id = id.ok_or("The stash is gone")?;
    let contents = contents(&root, &id)?;
    let view = editor
        .window_buffer_mut()
        .local_mut::<StatusView>()
        .ok_or(NOT_A_STATUS_BUFFER)?;
    view.show_stash(id, contents);
    status_view::sync(editor);
    let line = editor
        .window_buffer()
        .local::<StatusView>()
        .and_then(|view| {
            view.lines
                .iter()
                .position(|line| *line == Line::Stash(index))
        });
    if let Some(line) = line {
        editor.set_point_at(line, 0);
    }
    Ok(())
}

/// Reads a stash's files and diffs, to unfold it in the status buffer.
pub fn contents(root: &Path, id: &str) -> Result<StashContents, String> {
    let entry = read(root, &["-1", id])
        .into_iter()
        .next()
        .ok_or("The stash is gone")?;
    let printed = output(git(root).args([
        "-c",
        "core.quotePath=false",
        "stash",
        "show",
        "--patch",
        "--no-renames",
        "--include-untracked",
        "--no-color",
        "--no-ext-diff",
        id,
    ]))
    .map_err(|error| format!("git stash show: {error}"))?;
    Ok(StashContents {
        files: entry.files,
        diffs: split_patch(&String::from_utf8_lossy(&printed))
            .into_iter()
            .collect(),
    })
}

/// The working tree and the stash acted on: the one selected in the stash prompt, which
/// closes, or the one at point in the status buffer.
fn target(editor: &mut Editor) -> Result<(PathBuf, String), &'static str> {
    if let Some(minibuffer) = editor.minibuffer() {
        let name = minibuffer
            .selected_candidate()
            .filter(|candidate| candidate.expansion.is_some())
            .map(|candidate| candidate.value.clone())
            .ok_or("No stash selected")?;
        editor.cancel_minibuffer();
        let (root, _) = shown_status(editor)?;
        return Ok((root, name));
    }
    let (root, snapshot) = shown_status(editor)?;
    let point_line = editor.window_buffer().point_line();
    let view = editor
        .window_buffer()
        .local::<StatusView>()
        .ok_or(NOT_A_STATUS_BUFFER)?;
    let index = view
        .line(point_line)
        .and_then(Line::stash)
        .ok_or("No stash here")?;
    let name = snapshot
        .details
        .as_ref()
        .and_then(|details| details.stashes.get(index))
        .map(|stash| stash.name.clone())
        .ok_or("No stash here")?;
    Ok((root, name))
}

/// The stashes, newest first.
fn list(root: &Path) -> Result<Vec<StashEntry>, String> {
    if output(git(root).args(["rev-parse", "--verify", "--quiet", "refs/stash"])).is_err() {
        return Ok(Vec::new());
    }
    Ok(read(root, &["-g", "refs/stash"]))
}

/// Stashes `which` names, as `git log` takes them, with their files: tracked ones against
/// the commit stashed on, untracked ones from the commit Git keeps them in.
fn read(root: &Path, which: &[&str]) -> Vec<StashEntry> {
    let printed = output(
        git(root)
            .args(["-c", "core.quotePath=false", "log", RECORD, "-z"])
            .args([
                "--diff-merges=first-parent",
                "--no-renames",
                "--raw",
                "--numstat",
            ])
            .args(which)
            .arg("--"),
    )
    .unwrap_or_default();
    let mut entries = parse_log(&String::from_utf8_lossy(&printed));
    let untracked: Vec<String> = entries
        .iter()
        .filter_map(|(_, parent)| parent.clone())
        .collect();
    if !untracked.is_empty() {
        let printed = output(
            git(root)
                .args([
                    "-c",
                    "core.quotePath=false",
                    "show",
                    "--format=%x01%H",
                    "-z",
                ])
                .args(["--no-renames", "--numstat"])
                .args(&untracked)
                .arg("--"),
        )
        .unwrap_or_default();
        let mut files = parse_untracked(&String::from_utf8_lossy(&printed));
        for (entry, parent) in &mut entries {
            if let Some(added) = parent.as_ref().and_then(|parent| files.remove(parent)) {
                entry.files.extend(added);
                entry.files.sort_by(|a, b| a.path.cmp(&b.path));
            }
        }
    }
    entries.into_iter().map(|(entry, _)| entry).collect()
}

/// Reads [`RECORD`]s with `--raw --numstat -z`: each stash, and the commit holding its
/// untracked files, if any.
fn parse_log(printed: &str) -> Vec<(StashEntry, Option<String>)> {
    printed
        .split('\u{1}')
        .filter_map(|record| {
            let mut fields = record.split('\0');
            let name = fields.next()?.to_owned();
            let id = fields.next()?.to_owned();
            let time = fields.next()?.parse().unwrap_or_default();
            let untracked_parent = fields
                .next()?
                .split(' ')
                .nth(UNTRACKED_PARENT)
                .map(str::to_owned);
            let message = fields.next()?.to_owned();
            let mut files: Vec<StashFile> = Vec::new();
            while let Some(field) = fields.next() {
                let field = field.trim_start_matches('\n');
                if let Some(raw) = field.strip_prefix(':') {
                    let Some(path) = fields.next() else { break };
                    files.push(StashFile {
                        path: PathBuf::from(path),
                        state: raw_state(raw),
                        lines: None,
                    });
                } else if let Some((path, lines)) = numstat(field)
                    && let Some(file) = files.iter_mut().find(|file| file.path == path)
                {
                    file.lines = lines;
                }
            }
            let entry = StashEntry {
                name,
                id,
                message,
                time,
                files,
            };
            Some((entry, untracked_parent))
        })
        .collect()
}

/// Reads `git show --format=%x01%H --numstat -z` of commits of untracked files, by commit.
fn parse_untracked(printed: &str) -> HashMap<String, Vec<StashFile>> {
    printed
        .split('\u{1}')
        .filter_map(|record| {
            let mut fields = record.split('\0');
            let id = fields.next()?.to_owned();
            let files = fields
                .filter_map(|field| numstat(field.trim_start_matches('\n')))
                .map(|(path, lines)| StashFile {
                    path,
                    state: FileState::Untracked,
                    lines,
                })
                .collect();
            Some((id, files))
        })
        .collect()
}

/// `:100644 100644 422c2b7 f21fa3b M` without its colon.
fn raw_state(raw: &str) -> FileState {
    match raw.rsplit(' ').next() {
        Some("A") => FileState::Added,
        Some("D") => FileState::Deleted,
        _ => FileState::Modified,
    }
}

/// `1\t0\tpath`, `-\t-\tpath` for a binary file.
fn numstat(field: &str) -> Option<(PathBuf, Option<LineCounts>)> {
    let mut parts = field.splitn(3, '\t');
    let (added, removed, path) = (parts.next()?, parts.next()?, parts.next()?);
    let lines = added
        .parse()
        .ok()
        .zip(removed.parse().ok())
        .map(|(added, removed)| LineCounts { added, removed });
    Some((PathBuf::from(path), lines))
}

/// `1 file`, `3 files`.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn git_job(args: Vec<String>, name: &'static str) -> Job {
    Box::new(move |root| {
        // Git stashes untracked files with the pathspec `:/`, which literal pathspecs
        // turn into a file name; stashing takes no paths of ours.
        output(git(root).args(&args).env_remove("GIT_LITERAL_PATHSPECS"))
            .map(drop)
            .map_err(|error| format!("{name}: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stashes_list_their_tracked_and_untracked_files() {
        let printed = "\u{1}stash@{0}\0aaa\x001791137696\0p1 p2 p3\0WIP on main: d53a673 init\0\
\n:100644 100644 422c2b7 f21fa3b M\0f.txt\0:000000 100644 0000000 b77b4eb A\0g.txt\0\
1\t1\tf.txt\0-\t-\tg.txt\0\
\u{1}stash@{1}\0bbb\x001791137000\0p1 p2\0On main: first one\0\
\n:100644 100644 422c2b7 7be73ce D\0f.txt\0\
0\t2\tf.txt\0";
        let entries = parse_log(printed);
        assert_eq!(entries.len(), 2);
        let (first, untracked) = &entries[0];
        assert_eq!(
            (first.name.as_str(), first.id.as_str(), first.time),
            ("stash@{0}", "aaa", 1791137696)
        );
        assert_eq!(first.message, "WIP on main: d53a673 init");
        assert_eq!(untracked.as_deref(), Some("p3"));
        assert_eq!(
            first.files,
            [
                StashFile {
                    path: "f.txt".into(),
                    state: FileState::Modified,
                    lines: Some(LineCounts {
                        added: 1,
                        removed: 1
                    }),
                },
                StashFile {
                    path: "g.txt".into(),
                    state: FileState::Added,
                    lines: None,
                },
            ]
        );
        assert_eq!(entries[1].1, None);
        assert_eq!(entries[1].0.files[0].state, FileState::Deleted);

        let untracked = parse_untracked("\u{1}p3\0\n1\t0\tu.txt\0");
        assert_eq!(untracked["p3"][0].path, PathBuf::from("u.txt"));
        assert_eq!(untracked["p3"][0].state, FileState::Untracked);
    }
}
