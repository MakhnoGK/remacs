//! The history beside the status: the commits of all branches or of the current one,
//! searched by message, read a page at a time as they are scrolled to.

use std::rc::Rc;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use remacs_core::{
    Button, CommandResult, CommitRow, Completions, Editor, History, Prompt, RefChip, RefKind,
};

use crate::history::{Commit, HistoryWants, Log, PAGE};
use crate::repositories::Repositories;
use crate::status::{Head, Snapshot};
use crate::status_view::{NOT_A_STATUS_BUFFER, SHORT_ID, StatusView};

/// Commits given to the UI, more than any window fits.
const SHOWN: usize = 80;
/// Commits a key scrolls by.
const KEY_PAGE: isize = 20;

/// Which commits the history shows, and from which one on.
pub struct HistoryState {
    all: bool,
    search: Option<String>,
    first: usize,
    count: usize,
}

impl Default for HistoryState {
    fn default() -> Self {
        Self {
            all: true,
            search: None,
            first: 0,
            count: PAGE,
        }
    }
}

impl HistoryState {
    pub fn wants(&self) -> HistoryWants {
        HistoryWants {
            all: self.all,
            search: self.search.clone(),
            count: self.count,
        }
    }

    fn show(&mut self, all: bool, search: Option<String>) {
        *self = Self {
            all,
            search,
            ..Self::default()
        };
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
            .skip(state.first)
            .take(SHOWN)
            .map(|(commit, graph)| CommitRow {
                graph: graph.clone(),
                refs: chips(commit, &snapshot.head, base),
                subject: commit.subject.clone(),
                author: short_author(&commit.author),
                date: ago(now - commit.time),
                id: commit.id.chars().take(SHORT_ID).collect(),
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
        scopes: vec![
            scope_button("All branches", true, state.all),
            scope_button("Current", false, !state.all),
        ],
        search: state.search.clone(),
        commits,
        scrolled: state.first > 0,
        empty: empty.into(),
        scroll: Rc::new(scroll),
    }
}

fn scope_button(label: &str, all: bool, selected: bool) -> Button {
    Button {
        label: label.into(),
        selected,
        action: Rc::new(move |editor| {
            let view = window_view(editor)?;
            let search = view.history.search.clone();
            view.history.show(all, search);
            Ok(())
        }),
    }
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
    let search = view.history.search.clone();
    let all = !view.history.all;
    view.history.show(all, search);
    Ok(())
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
            let all = view.history.all;
            let search = Some(answer.trim().to_owned()).filter(|search| !search.is_empty());
            view.history.show(all, search);
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
