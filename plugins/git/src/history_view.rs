//! The history beside the status: the commits of all branches or of the current one,
//! searched by message, read a page at a time as they are scrolled to.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use remacs_core::{
    Action, Button, CommandResult, CommitRow, Completions, Editor, History, Prompt, RefChip,
    RefKind,
};

use crate::history::{self, ChangedFile, Commit, HistoryWants, Log, PAGE};
use crate::repositories::{Repositories, working_tree};
use crate::status::{Head, Snapshot};
use crate::status_view::{Line, NOT_A_STATUS_BUFFER, SHORT_ID, StatusView, Tab};
use crate::{commands, commit_view, reads};

/// Commits given to the UI, more than any window fits.
const SHOWN: usize = 80;
/// Commits a key scrolls by.
const KEY_PAGE: isize = 20;

/// Which commits the history shows, and from which one on.
pub struct HistoryState {
    all: bool,
    search: Option<String>,
    file: Option<PathBuf>,
    first: usize,
    count: usize,
    /// The commit the keys act on, by its place in the log.
    selected: Option<usize>,
    /// The files of a commit, once read, with its id: the selected one's, or the one
    /// selected before while the selected one's are read.
    files: Option<(String, Arc<[ChangedFile]>)>,
    /// The commit whose files are being read.
    reading: Option<String>,
}

impl Default for HistoryState {
    fn default() -> Self {
        Self {
            all: true,
            search: None,
            file: None,
            first: 0,
            count: PAGE,
            selected: None,
            files: None,
            reading: None,
        }
    }
}

impl HistoryState {
    /// Of every branch, else of the current one.
    pub fn all(&self) -> bool {
        self.all
    }

    pub fn set_all(&mut self, all: bool) {
        if all != self.all {
            self.show(all, self.search.clone(), self.file.clone());
        }
    }

    pub fn files(&self) -> Option<&[ChangedFile]> {
        self.files.as_ref().map(|(_, files)| &files[..])
    }

    pub fn wants(&self) -> HistoryWants {
        HistoryWants {
            all: self.all,
            search: self.search.clone(),
            file: self.file.clone(),
            count: self.count,
        }
    }

    fn show(&mut self, all: bool, search: Option<String>, file: Option<PathBuf>) {
        *self = Self {
            all,
            search,
            file,
            ..Self::default()
        };
    }

    /// Shows only the commits that changed `file`, relative to the root.
    pub fn show_file(&mut self, file: PathBuf) {
        self.show(self.all, None, Some(file));
    }

    /// Moves the selection by `rows`, or selects the first commit shown; scrolls to it
    /// when it is above the first one shown or past those given to the UI.
    fn select(&mut self, rows: isize, log: &Log) {
        let Some(last) = log.commits.len().checked_sub(1) else {
            return;
        };
        let selected = match self.selected {
            Some(selected) => selected.saturating_add_signed(rows).min(last),
            None => self.first.min(last),
        };
        self.selected = Some(selected);
        if selected < self.first {
            self.first = selected;
        } else if selected >= self.first + SHOWN {
            self.first = selected + 1 - SHOWN;
        }
        self.scroll(0, log);
    }

    /// Moves the first commit shown by `rows`, asking for another page when it nears
    /// the last one read.
    fn scroll(&mut self, rows: isize, log: &Log) {
        let read = log.commits.len();
        self.first = self
            .first
            .saturating_add_signed(rows)
            .min(read.saturating_sub(1));
        if !log.complete && self.first + SHOWN >= read {
            self.count = self.count.max(read + PAGE);
        }
    }
}

/// The history of the status buffer's tree, from the first commit scrolled to.
pub fn history(view: &StatusView, snapshot: &Snapshot) -> History {
    let state = &view.history;
    let log = snapshot.history.as_deref();
    let base = snapshot
        .details
        .as_ref()
        .and_then(|details| details.base.as_ref())
        .map(|base| base.name.as_str());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    let commits = log.map_or_else(Vec::new, |log| {
        log.commits
            .iter()
            .zip(&log.graph)
            .enumerate()
            .skip(state.first)
            .take(SHOWN)
            .map(|(index, (commit, graph))| CommitRow {
                graph: graph.clone(),
                refs: chips(commit, &snapshot.head, base),
                subject: commit.subject.clone(),
                author: short_author(&commit.author),
                date: ago(now - commit.time),
                id: commit.id.chars().take(SHORT_ID).collect(),
                open: open_action(view, &commit.id, index),
            })
            .collect()
    });
    let empty = match log {
        _ if snapshot.is_unborn() => "No commits yet",
        None => "Reading the history…",
        Some(_) if state.search.is_some() => "No commit message matches",
        Some(_) => "No commits",
    };
    History {
        search: state.search.clone(),
        file: state.file.as_ref().map(|file| Button {
            label: file.display().to_string(),
            selected: true,
            action: Rc::new(all_files),
        }),
        selected: state
            .selected
            .and_then(|selected| selected.checked_sub(state.first))
            .filter(|&selected| selected < commits.len()),
        commits,
        scrolled: state.first > 0,
        empty: empty.into(),
        scroll: Rc::new(scroll),
    }
}

/// Selects the commit at `index` of the log and shows it, its file unfolded in the log
/// of a file.
fn open_action(view: &StatusView, id: &str, index: usize) -> Action {
    let (root, id, file) = (view.root.clone(), id.to_owned(), view.history.file.clone());
    Rc::new(move |editor| {
        window_view(editor)?.history.selected = Some(index);
        commit_view::open(editor, &root, &id, file.as_deref())
    })
}

/// Reads the files of the commit selected in the window's history, unless they are read
/// or being read; the first commit shown is selected when none is.
pub fn read_selected_files(editor: &mut Editor) {
    let Some(log) = window_log(editor) else {
        return;
    };
    let Ok(view) = window_view(editor) else {
        return;
    };
    let state = &mut view.history;
    let selected = *state.selected.get_or_insert(state.first);
    let Some(commit) = log.commits.get(selected) else {
        return;
    };
    let read = state.files.as_ref().is_some_and(|(id, _)| *id == commit.id);
    if read || state.reading.as_ref() == Some(&commit.id) {
        return;
    }
    state.reading = Some(commit.id.clone());
    let (root, id) = (view.root.clone(), commit.id.clone());
    let read_root = root.clone();
    reads::spawn_quietly(
        editor,
        move || {
            history::read_files(&read_root, &id)
                .map(|files| (id, files))
                .map_err(|error| format!("git show: {error}"))
        },
        move |editor, (id, files)| {
            show_files(editor, &root, id, files);
            Ok(())
        },
    );
}

/// Shows the files of commit `id` beside the history of `root`'s status, unless another
/// commit was selected since.
fn show_files(editor: &mut Editor, root: &Path, id: String, files: Vec<ChangedFile>) {
    let view = editor
        .buffers_mut()
        .iter_mut()
        .filter_map(|buffer| buffer.local_mut::<StatusView>())
        .find(|view| view.root == root);
    if let Some(view) = view.filter(|view| view.history.reading.as_ref() == Some(&id)) {
        view.history.reading = None;
        view.history.files = Some((id, files.into()));
        view.redraw_from_top();
    }
}

/// Shows the commit whose files are listed, at the file at point if there is one.
pub fn open_listed(editor: &mut Editor) -> CommandResult {
    let point_line = editor.window_buffer().point_line();
    let view = window_view(editor)?;
    let Some((id, files)) = view.history.files.clone() else {
        return open_commit(editor);
    };
    let file = match view.line(point_line) {
        Some(Line::CommitFile(index)) => files.get(*index).map(|file| file.path.clone()),
        _ => view.history.file.clone(),
    };
    let root = view.root.clone();
    commit_view::open(editor, &root, &id, file.as_deref())
}

/// Branches and tags at `commit`: the one checked out, the base, other branches, remote
/// ones, tags.
fn chips(commit: &Commit, head: &Head, base: Option<&str>) -> Vec<RefChip> {
    let current = match head {
        Head::Branch(name) => Some(name.as_str()),
        _ => None,
    };
    let mut chips: Vec<RefChip> = Vec::new();
    if commit.is_head && current.is_none() {
        chips.push(RefChip {
            name: "HEAD".into(),
            kind: RefKind::Head,
        });
    }
    for name in &commit.refs {
        let (short, kind) = if let Some(branch) = name.strip_prefix("refs/heads/") {
            let kind = match Some(branch) {
                name if name == current => RefKind::Head,
                name if name == base => RefKind::Main,
                _ => RefKind::Branch,
            };
            (branch, kind)
        } else if let Some(remote) = name.strip_prefix("refs/remotes/") {
            let kind = if Some(remote) == base {
                RefKind::Main
            } else {
                RefKind::Remote
            };
            (remote, kind)
        } else if let Some(tag) = name.strip_prefix("refs/tags/") {
            (tag, RefKind::Tag)
        } else {
            continue;
        };
        chips.push(RefChip {
            name: short.into(),
            kind,
        });
    }
    chips.sort_by_key(|chip| match chip.kind {
        RefKind::Head => 0,
        RefKind::Main => 1,
        RefKind::Branch => 2,
        RefKind::Remote => 3,
        RefKind::Tag => 4,
    });
    chips
}

/// `Hryhorii K.` for `Hryhorii Khyzhniakov`.
pub fn short_author(name: &str) -> String {
    let mut words = name.split_whitespace();
    match (
        words.next(),
        words.next_back().and_then(|last| last.chars().next()),
    ) {
        (Some(first), Some(initial)) => format!("{first} {initial}."),
        _ => name.to_owned(),
    }
}

/// `12 min ago`, `yesterday`, `3 weeks ago`.
pub fn ago(seconds: i64) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    const TWO_DAYS: i64 = 2 * DAY;
    const WEEK: i64 = 7 * DAY;
    const MONTH: i64 = 30 * DAY;
    const YEAR: i64 = 365 * DAY;
    let count = |unit: i64, singular: &str, plural: &str| {
        let count = seconds / unit;
        format!("{count} {} ago", if count == 1 { singular } else { plural })
    };
    // A clock running behind dates commits ahead: they are just made.
    match seconds {
        i64::MIN..MINUTE => "just now".into(),
        MINUTE..HOUR => count(MINUTE, "min", "min"),
        HOUR..DAY => count(HOUR, "h", "h"),
        DAY..TWO_DAYS => "yesterday".into(),
        TWO_DAYS..WEEK => count(DAY, "day", "days"),
        WEEK..MONTH => count(WEEK, "week", "weeks"),
        MONTH..YEAR => count(MONTH, "month", "months"),
        _ => count(YEAR, "year", "years"),
    }
}

/// Shows the commits of all branches, or only those of the current one, like Emacs
/// `magit-log-all` and `magit-log-current`.
pub fn toggle_scope(editor: &mut Editor) -> CommandResult {
    let view = window_view(editor)?;
    let all = !view.history.all;
    view.history.set_all(all);
    Ok(())
}

/// Shows the status with only the commits that changed the visited file in its
/// history, followed through renames, like Emacs `magit-log-buffer-file`.
pub fn file_log(editor: &mut Editor) -> CommandResult {
    let path = editor
        .window_buffer()
        .path()
        .ok_or("Not visiting a file")?
        .to_path_buf();
    let root = path
        .parent()
        .and_then(working_tree)
        .ok_or("Not in a Git working tree")?;
    let relative = path
        .strip_prefix(&root)
        .map_err(|_| "Not in a Git working tree")?
        .to_path_buf();
    commands::status(editor)?;
    let view = window_view(editor)?;
    view.show_tab(Tab::History, view.history.all());
    view.history.show_file(relative);
    Ok(())
}

/// Whether the window's buffer visits a file, whose log `git-log-file` shows.
pub fn visits_file(editor: &Editor) -> bool {
    editor.window_buffer().path().is_some()
}

/// Shows the commits of every file again.
pub fn all_files(editor: &mut Editor) -> CommandResult {
    let view = window_view(editor)?;
    if view.history.file.is_none() {
        return Err("The history already shows every file".into());
    }
    let (all, search) = (view.history.all, view.history.search.clone());
    view.history.show(all, search, None);
    Ok(())
}

/// Selects the next older commit of the history.
pub fn next_commit(editor: &mut Editor) -> CommandResult {
    select(editor, 1)
}

/// Selects the next newer commit of the history.
pub fn previous_commit(editor: &mut Editor) -> CommandResult {
    select(editor, -1)
}

fn select(editor: &mut Editor, rows: isize) -> CommandResult {
    let log = window_log(editor).ok_or("Still reading the history")?;
    window_view(editor)?.history.select(rows, &log);
    Ok(())
}

/// Shows what the selected commit of the history changed, or the first one shown.
pub fn open_commit(editor: &mut Editor) -> CommandResult {
    let log = window_log(editor).ok_or("Still reading the history")?;
    let view = window_view(editor)?;
    let index = view.history.selected.unwrap_or(view.history.first);
    let commit = log.commits.get(index).ok_or("No commits")?;
    view.history.selected = Some(index);
    let (root, file) = (view.root.clone(), view.history.file.clone());
    commit_view::open(editor, &root, &commit.id, file.as_deref())
}

/// Reads what to search the commit messages for; nothing shows every commit again.
pub fn search(editor: &mut Editor) -> CommandResult {
    let current = window_view(editor)?.history.search.clone();
    let prompt = Prompt {
        label: "Search history".into(),
        placeholder: "Words in commit messages",
        initial_input: current.unwrap_or_default(),
        hints: &[("↵", "Search"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        |editor, answer| {
            let view = window_view(editor)?;
            let (all, file) = (view.history.all, view.history.file.clone());
            let search = Some(answer.trim().to_owned()).filter(|search| !search.is_empty());
            view.history.show(all, search, file);
            Ok(())
        },
    )
}

/// Scrolls the history to older commits, like Emacs `scroll-other-window`.
pub fn next_page(editor: &mut Editor) -> CommandResult {
    scroll(editor, KEY_PAGE)
}

/// Scrolls the history to newer commits.
pub fn previous_page(editor: &mut Editor) -> CommandResult {
    scroll(editor, -KEY_PAGE)
}

fn scroll(editor: &mut Editor, rows: isize) -> CommandResult {
    let log = window_log(editor).ok_or("Still reading the history")?;
    window_view(editor)?.history.scroll(rows, &log);
    Ok(())
}

fn window_log(editor: &Editor) -> Option<Arc<Log>> {
    let view = editor.window_buffer().local::<StatusView>()?;
    editor
        .find_plugin_state::<Repositories>()?
        .get(&view.root)?
        .snapshot
        .as_ref()?
        .history
        .clone()
}

fn window_view(editor: &mut Editor) -> Result<&mut StatusView, &'static str> {
    editor
        .window_buffer_mut()
        .local_mut::<StatusView>()
        .ok_or(NOT_A_STATUS_BUFFER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_authors_are_said_briefly() {
        let days = |count: i64| count * 24 * 3600;
        let said: Vec<String> = [
            30,
            12 * 60,
            3600,
            days(1) + 3600,
            days(2),
            days(8),
            days(21),
            days(60),
            days(400),
        ]
        .into_iter()
        .map(ago)
        .collect();
        assert_eq!(
            said,
            [
                "just now",
                "12 min ago",
                "1 h ago",
                "yesterday",
                "2 days ago",
                "1 week ago",
                "3 weeks ago",
                "2 months ago",
                "1 year ago",
            ]
        );
        assert_eq!(short_author("Hryhorii Khyzhniakov"), "Hryhorii K.");
        assert_eq!(short_author("Olena Petrivna Koval"), "Olena K.");
        assert_eq!(short_author("ci-bot"), "ci-bot");
    }

    #[test]
    fn scrolling_near_the_last_commit_read_asks_for_another_page() {
        let log = |commits: usize, complete: bool| Log {
            wants: HistoryState::default().wants(),
            commits: (0..commits)
                .map(|index| Commit {
                    id: index.to_string(),
                    parents: Vec::new(),
                    author: String::new(),
                    time: 0,
                    subject: String::new(),
                    refs: Vec::new(),
                    is_head: false,
                })
                .collect(),
            graph: Vec::new(),
            complete,
        };
        let mut state = HistoryState::default();
        state.scroll(20, &log(PAGE, false));
        assert_eq!((state.first, state.count), (20, PAGE));
        state.scroll(PAGE as isize, &log(PAGE, false));
        assert_eq!((state.first, state.count), (PAGE - 1, 2 * PAGE));

        let mut state = HistoryState::default();
        state.scroll(PAGE as isize, &log(PAGE, true));
        assert_eq!(state.count, PAGE);
        state.scroll(-1000, &log(0, true));
        assert_eq!(state.first, 0);
    }
}
