//! The commits of a repository, newest first, read with `git log` and laid out in lanes
//! like `git log --graph`.

use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use remacs_core::{FileState, GraphEdge, GraphNode, GraphRow, LineCounts};

use crate::command::{git, output};
use crate::status::{Snapshot, parse_numstat};
use crate::status_view::side_state;

/// Commits read at first, and more each time the reader nears the last one read.
pub const PAGE: usize = 200;
const HEAD_COLOR: usize = 0;
const BASE_COLOR: usize = 1;
const FIELD_SEPARATOR: char = '\x1f';
const FORMAT: &str = "--format=%H%x1f%P%x1f%an%x1f%at%x1f%D%x1f%s";

/// Which commits to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryWants {
    /// Of every branch, remote branch and tag, or only those of the commit checked out.
    pub all: bool,
    /// Only those whose message holds this, ignoring case.
    pub search: Option<String>,
    /// Only those that changed this file, relative to the root, followed through renames.
    pub file: Option<PathBuf>,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Log {
    /// What was read.
    pub wants: HistoryWants,
    pub commits: Vec<Commit>,
    /// One row per commit.
    pub graph: Vec<GraphRow>,
    /// Fewer commits were found than asked for: there are no more.
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    /// Seconds since the epoch, as the author dated it.
    pub time: i64,
    pub subject: String,
    /// Full names, e.g. `refs/heads/main`, `refs/tags/v1.0`.
    pub refs: Vec<String>,
    /// Checked out.
    pub is_head: bool,
}

/// Keeps the last log read, as the commits change only when a ref does.
#[derive(Default)]
pub struct HistoryCache {
    last: Option<(HistoryWants, String, Arc<Log>)>,
}

impl HistoryCache {
    pub fn read(
        &mut self,
        root: &Path,
        wants: &HistoryWants,
        snapshot: &Snapshot,
    ) -> io::Result<Arc<Log>> {
        let base = snapshot
            .details
            .as_ref()
            .and_then(|details| details.base.as_ref())
            .map(|base| base.name.as_str());
        let refs = output(git(root).args([
            "for-each-ref",
            "--format=%(objectname) %(refname)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ]))?;
        let fingerprint = format!(
            "{:?}\n{base:?}\n{}",
            snapshot.head,
            String::from_utf8_lossy(&refs)
        );
        if let Some((last_wants, last_fingerprint, log)) = &self.last
            && last_wants == wants
            && *last_fingerprint == fingerprint
        {
            return Ok(log.clone());
        }
        let log = Arc::new(read(root, wants, base)?);
        self.last = Some((wants.clone(), fingerprint, log.clone()));
        Ok(log)
    }
}

fn read(root: &Path, wants: &HistoryWants, base: Option<&str>) -> io::Result<Log> {
    let mut command = git(root);
    command.args([
        "log",
        "--date-order",
        "--decorate=full",
        "--no-color",
        "--no-show-signature",
        "-z",
        FORMAT,
        &format!("--max-count={}", wants.count),
    ]);
    if let Some(search) = &wants.search {
        command.args(["-i", "-F", &format!("--grep={search}")]);
    }
    if wants.all {
        command.args(["--branches", "--remotes", "--tags"]);
    }
    if wants.file.is_some() {
        command.arg("--follow");
    }
    command.args(["HEAD", "--"]).args(&wants.file);
    let commits = parse_log(&String::from_utf8_lossy(&output(&mut command)?));
    let base_tip = base.and_then(|base| {
        let names = [format!("refs/heads/{base}"), format!("refs/remotes/{base}")];
        commits
            .iter()
            .find(|commit| commit.refs.iter().any(|name| names.contains(name)))
            .map(|commit| commit.id.as_str())
    });
    // Found by their messages or files, commits are not each other's parents.
    let connected = wants.search.is_none() && wants.file.is_none();
    let graph = layout(&commits, base_tip, connected);
    Ok(Log {
        wants: wants.clone(),
        complete: commits.len() < wants.count,
        commits,
        graph,
    })
}

/// Commits of `git log -z` with [`FORMAT`].
pub fn parse_log(printed: &str) -> Vec<Commit> {
    printed
        .split('\0')
        .filter_map(|record| {
            let mut fields = record.trim_start_matches('\n').split(FIELD_SEPARATOR);
            let id = fields.next().filter(|id| !id.is_empty())?.to_owned();
            let parents = fields
                .next()?
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            let author = fields.next()?.to_owned();
            let time = fields.next()?.parse().unwrap_or_default();
            let (refs, is_head) = parse_decorations(fields.next()?);
            let subject = fields.collect::<Vec<_>>().join(" ");
            Some(Commit {
                id,
                parents,
                author,
                time,
                subject,
                refs,
                is_head,
            })
        })
        .collect()
}

/// `HEAD -> refs/heads/main, tag: refs/tags/v1, refs/remotes/origin/main`, with a
/// remote's symbolic `HEAD` left out.
fn parse_decorations(decorations: &str) -> (Vec<String>, bool) {
    let mut is_head = false;
    let mut refs = Vec::new();
    for decoration in decorations.split(", ").filter(|each| !each.is_empty()) {
        let name = match decoration.strip_prefix("HEAD -> ") {
            Some(branch) => {
                is_head = true;
                branch
            }
            None if decoration == "HEAD" => {
                is_head = true;
                continue;
            }
            None => decoration.strip_prefix("tag: ").unwrap_or(decoration),
        };
        if !(name.starts_with("refs/remotes/") && name.ends_with("/HEAD")) {
            refs.push(name.to_owned());
        }
    }
    (refs, is_head)
}

/// A column of the graph, waiting for the commit it leads down to.
struct Lane<'a> {
    awaits: &'a str,
    color: usize,
}

/// Lays commits, newest first, out in lanes: each sits in a lane a child left for it,
/// the base's lane before the others, else in a new one, and leaves lanes for its
/// parents. Unconnected, each commit sits in the first lane alone.
pub fn layout(commits: &[Commit], base_tip: Option<&str>, connected: bool) -> Vec<GraphRow> {
    let mut lanes: Vec<Option<Lane>> = Vec::new();
    let mut next_color = BASE_COLOR + 1;
    let mut new_color = |id: &str, is_head: bool| match () {
        _ if is_head => HEAD_COLOR,
        _ if Some(id) == base_tip => BASE_COLOR,
        _ if !connected => BASE_COLOR + 1,
        _ => {
            next_color += 1;
            next_color - 1
        }
    };
    let mut rows = Vec::with_capacity(commits.len());
    for commit in commits {
        let awaiting: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(_, lane)| lane.as_ref().is_some_and(|lane| lane.awaits == commit.id))
            .map(|(index, _)| index)
            .collect();
        let continued = awaiting
            .iter()
            .copied()
            .find(|&index| {
                lanes[index]
                    .as_ref()
                    .is_some_and(|lane| lane.color == BASE_COLOR)
            })
            .or(awaiting.first().copied());
        let upper: Vec<GraphEdge> = lanes
            .iter()
            .enumerate()
            .filter_map(|(index, lane)| {
                let lane = lane.as_ref()?;
                Some(GraphEdge {
                    from: index,
                    to: if lane.awaits == commit.id {
                        continued.unwrap_or(index)
                    } else {
                        index
                    },
                    color: lane.color,
                })
            })
            .collect();
        let (lane, color) = match continued {
            Some(index) => (index, lanes[index].as_ref().map_or(0, |lane| lane.color)),
            None => (free_lane(&mut lanes), new_color(&commit.id, commit.is_head)),
        };
        for &index in &awaiting {
            lanes[index] = None;
        }
        let mut lower: Vec<GraphEdge> = lanes
            .iter()
            .enumerate()
            .filter_map(|(index, lane)| {
                let color = lane.as_ref()?.color;
                Some(GraphEdge {
                    from: index,
                    to: index,
                    color,
                })
            })
            .collect();
        let parents = if connected { &commit.parents[..] } else { &[] };
        for (number, parent) in parents.iter().enumerate() {
            let waiting = lanes
                .iter()
                .position(|lane| lane.as_ref().is_some_and(|lane| lane.awaits == *parent));
            let (target, target_color) = match waiting {
                Some(index) if number > 0 => (index, lanes[index].as_ref().map_or(0, |l| l.color)),
                _ => {
                    let target = if number == 0 {
                        lane
                    } else {
                        free_lane(&mut lanes)
                    };
                    let target_color = if number == 0 {
                        color
                    } else {
                        new_color(parent, false)
                    };
                    lanes[target] = Some(Lane {
                        awaits: parent,
                        color: target_color,
                    });
                    (target, target_color)
                }
            };
            lower.push(GraphEdge {
                from: lane,
                to: target,
                color: if number == 0 { color } else { target_color },
            });
        }
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        rows.push(GraphRow {
            lane,
            color,
            node: match () {
                _ if commit.is_head => GraphNode::Head,
                _ if commit.parents.len() > 1 => GraphNode::Merge,
                _ => GraphNode::Commit,
            },
            upper,
            lower,
        });
    }
    rows
}

/// The first lane no one waits in, made if there is none; it must be filled or freed.
fn free_lane(lanes: &mut Vec<Option<Lane>>) -> usize {
    match lanes.iter().position(Option::is_none) {
        Some(index) => index,
        None => {
            lanes.push(None);
            lanes.len() - 1
        }
    }
}

/// A file a commit changed, against its first parent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: PathBuf,
    pub state: FileState,
    /// None for a binary file.
    pub lines: Option<LineCounts>,
    /// Where a renamed or copied file came from.
    pub original: Option<PathBuf>,
}

/// The files commit `id` changed, without reading their diffs.
pub fn read_files(root: &Path, id: &str) -> io::Result<Vec<ChangedFile>> {
    let printed = output(
        git(root)
            .args(["show", "--format=", "--raw", "--numstat", "-z", "-M"])
            .args(["--diff-merges=first-parent", "--no-color", id, "--"]),
    )?;
    Ok(parse_files(&printed))
}

/// Reads `git show --raw --numstat -z`: the raw records, `:<modes> <ids> <state>` and
/// the path, or both paths for a rename or copy; then the line counts.
fn parse_files(printed: &[u8]) -> Vec<ChangedFile> {
    let mut rest = printed;
    let mut files = Vec::new();
    while rest.starts_with(b":") {
        let raw = take_record(&mut rest);
        let letter = raw.rsplit(|&byte| byte == b' ').next();
        let letter = letter
            .and_then(|state| state.first())
            .copied()
            .unwrap_or(b'M');
        let first = path(take_record(&mut rest));
        let (path, original) = match letter {
            b'R' | b'C' => (path(take_record(&mut rest)), Some(first)),
            _ => (first, None),
        };
        files.push(ChangedFile {
            path,
            state: side_state(letter),
            lines: None,
            original,
        });
    }
    for (path, lines) in parse_numstat(rest) {
        if let Some(file) = files.iter_mut().find(|file| file.path == path) {
            file.lines = lines;
        }
    }
    files
}

/// The record `rest` starts with, up to its NUL, which `rest` is moved past.
fn take_record<'a>(rest: &mut &'a [u8]) -> &'a [u8] {
    let (record, after) = rest.split_at(
        rest.iter()
            .position(|&byte| byte == 0)
            .unwrap_or(rest.len()),
    );
    *rest = after.get(1..).unwrap_or_default();
    record
}

fn path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsStr::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commits_files_carry_their_states_renames_and_line_counts() {
        let printed = b":000000 100644 0000000 d5d0b8b A\0bin\0\
:100644 100644 7898192 7898192 R100\0a\0c\0\
:100644 100644 1111111 2222222 M\0src/lib.rs\0\
-\t-\tbin\0\
0\t0\t\0a\0c\0\
3\t1\tsrc/lib.rs\0";
        let files = parse_files(printed);
        let counts = |added, removed| Some(LineCounts { added, removed });
        assert_eq!(
            files,
            [
                ChangedFile {
                    path: "bin".into(),
                    state: FileState::Added,
                    lines: None,
                    original: None,
                },
                ChangedFile {
                    path: "c".into(),
                    state: FileState::Renamed,
                    lines: counts(0, 0),
                    original: Some("a".into()),
                },
                ChangedFile {
                    path: "src/lib.rs".into(),
                    state: FileState::Modified,
                    lines: counts(3, 1),
                    original: None,
                },
            ]
        );
        assert!(parse_files(b"").is_empty());
    }

    fn commit(id: &str, parents: &[&str], is_head: bool) -> Commit {
        Commit {
            id: id.into(),
            parents: parents.iter().map(|parent| (*parent).into()).collect(),
            author: String::new(),
            time: 0,
            subject: String::new(),
            refs: Vec::new(),
            is_head,
        }
    }

    fn edge(from: usize, to: usize, color: usize) -> GraphEdge {
        GraphEdge { from, to, color }
    }

    #[test]
    fn reads_commits_with_their_parents_and_refs() {
        let printed = "a1\x1fb2 c3\x1fTaras M\x1f1700000000\x1fHEAD -> refs/heads/main, \
                       refs/remotes/origin/HEAD, refs/remotes/origin/main, tag: refs/tags/v1\
                       \x1fMerge branch 'fix'\0\nb2\x1f\x1fOlena\x1f1600000000\x1fHEAD\x1fFirst\0";
        let commits = parse_log(printed);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].parents, ["b2", "c3"]);
        assert_eq!(
            commits[0].refs,
            [
                "refs/heads/main",
                "refs/remotes/origin/main",
                "refs/tags/v1"
            ]
        );
        assert!(commits[0].is_head);
        assert_eq!(commits[0].subject, "Merge branch 'fix'");
        assert_eq!(commits[1].time, 1_600_000_000);
        assert!(commits[1].parents.is_empty() && commits[1].refs.is_empty());
        assert!(commits[1].is_head);
    }

    #[test]
    fn a_merged_branch_takes_a_lane_of_its_own_until_it_forks_off() {
        let commits = [
            commit("merge", &["main", "topic"], true),
            commit("topic", &["fork"], false),
            commit("main", &["fork"], false),
            commit("fork", &["root"], false),
            commit("root", &[], false),
        ];
        let rows = layout(&commits, None, true);
        assert_eq!(rows[0].node, GraphNode::Head);
        assert_eq!((rows[0].lane, rows[0].color), (0, HEAD_COLOR));
        assert!(rows[0].upper.is_empty());
        assert_eq!(rows[0].lower, [edge(0, 0, 0), edge(0, 1, 2)]);
        assert_eq!((rows[1].lane, rows[1].color), (1, 2));
        assert_eq!(rows[1].upper, [edge(0, 0, 0), edge(1, 1, 2)]);
        assert_eq!(rows[2].lane, 0);
        assert_eq!(rows[3].upper, [edge(0, 0, 0), edge(1, 0, 2)]);
        assert_eq!(rows[3].lower, [edge(0, 0, 0)]);
        assert_eq!(rows[4].lower, []);
    }

    #[test]
    fn shared_history_goes_on_in_the_base_lane() {
        let commits = [
            commit("feature", &["fork"], true),
            commit("main", &["fork"], false),
            commit("fork", &[], false),
        ];
        let rows = layout(&commits, Some("main"), true);
        assert_eq!((rows[1].lane, rows[1].color), (1, BASE_COLOR));
        assert_eq!((rows[2].lane, rows[2].color), (1, BASE_COLOR));
        assert_eq!(
            rows[2].upper,
            [edge(0, 1, HEAD_COLOR), edge(1, 1, BASE_COLOR)]
        );
    }

    #[test]
    fn found_commits_are_not_connected() {
        let commits = [commit("b", &["a"], false), commit("a", &[], false)];
        let rows = layout(&commits, None, false);
        assert!(rows.iter().all(|row| row.lane == 0 && row.lower.is_empty()));
    }
}
