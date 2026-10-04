//! The status buffer, like Emacs magit-status: facts about the branch, then the changed
//! files by section, each line one row, a file's diff unfolded under it.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_core::{
    Buffer, Button, DiffRow, Editor, Fact, FactValue, FileState, Icon, LineCounts, Outline,
    OutlineRow, RefChip, RefKind, StatusItem, StatusSegment, Tone,
};
use remacs_text::Text;

use crate::diff::FileDiff;
use crate::history_view::{self, HistoryState};
use crate::integrate;
use crate::monitor::Wants;
use crate::repositories::Repositories;
use crate::status::{DiffKey, Head, Snapshot};

pub const GIT_MODE: &str = "Git";
pub const TOGGLE: &str = "git-toggle";
pub const VISIT: &str = "git-visit";
pub const NOT_A_STATUS_BUFFER: &str = "Not a Git status buffer";
/// As the design has them; TAB, k and RET are left to be found, as in magit.
const HINTS: &[(&str, &str)] = &[
    ("c", "Commit"),
    ("b", "Branch"),
    ("m", "Merge"),
    ("r", "Rebase"),
    ("s", "Stage"),
    ("u", "Unstage"),
];
/// Diff lines shown of one file at most, so a huge change does not swamp the buffer.
const MAX_DIFF_LINES: usize = 2000;
pub const SHORT_ID: usize = 7;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Section {
    Unmerged,
    Staged,
    Unstaged,
    Untracked,
    Stashes,
}

impl Section {
    const FILES: [Section; 4] = [
        Section::Unmerged,
        Section::Staged,
        Section::Unstaged,
        Section::Untracked,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::Unmerged => "Unmerged",
            Section::Staged => "Staged",
            Section::Unstaged => "Unstaged",
            Section::Untracked => "Untracked",
            Section::Stashes => "Stashes",
        }
    }

    /// Whether its files have a diff to unfold.
    pub fn has_diffs(self) -> bool {
        matches!(self, Section::Staged | Section::Unstaged)
    }
}

/// What a line of the status buffer stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Heading(Section),
    File(Section, PathBuf),
    /// A hunk's header, with `line` `None`, or one of its lines.
    Diff {
        section: Section,
        path: PathBuf,
        hunk: usize,
        line: Option<usize>,
    },
    /// In place of a diff that has no lines, e.g. a binary file's.
    NoDiff(Section, PathBuf),
    Stash(usize),
}

impl Line {
    /// The file the line is, or is part of the diff of.
    pub fn file(&self) -> Option<(Section, &Path)> {
        match self {
            Line::File(section, path)
            | Line::Diff { section, path, .. }
            | Line::NoDiff(section, path) => Some((*section, path)),
            Line::Heading(_) | Line::Stash(_) => None,
        }
    }
}

/// The state of a status buffer.
pub struct StatusView {
    pub root: PathBuf,
    folded: HashSet<Section>,
    open: HashSet<(Section, PathBuf)>,
    pub lines: Vec<Line>,
    /// The snapshot generation drawn; `None` to draw again.
    drawn: Option<u64>,
    pub history: HistoryState,
}

impl StatusView {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            folded: HashSet::from([Section::Stashes]),
            open: HashSet::new(),
            lines: Vec::new(),
            drawn: None,
            history: HistoryState::default(),
        }
    }

    pub fn toggle_section(&mut self, section: Section) {
        if !self.folded.remove(&section) {
            self.folded.insert(section);
        }
        self.drawn = None;
    }

    pub fn toggle_file(&mut self, section: Section, path: &Path) {
        let key = (section, path.to_path_buf());
        if !self.open.remove(&key) {
            self.open.insert(key);
        }
        self.drawn = None;
    }

    pub fn line(&self, index: usize) -> Option<&Line> {
        self.lines.get(index)
    }
}

/// A file of a section, as the section sees it.
pub struct SectionFile {
    pub path: PathBuf,
    pub state: FileState,
    pub lines: Option<LineCounts>,
    /// Where it was before it was renamed.
    pub original: Option<PathBuf>,
}

/// The files of `section`, by path.
pub fn section_files(snapshot: &Snapshot, section: Section) -> Vec<SectionFile> {
    let mut files: Vec<SectionFile> = snapshot
        .files
        .iter()
        .filter_map(|(path, entry)| {
            let (state, lines) = match section {
                Section::Unmerged if entry.conflicted => (FileState::Conflicted, None),
                _ if entry.conflicted => return None,
                Section::Staged if !matches!(entry.staged, b'.' | b'?') => {
                    (side_state(entry.staged), entry.staged_lines)
                }
                Section::Unstaged if entry.staged != b'?' && entry.unstaged != b'.' => {
                    (side_state(entry.unstaged), entry.unstaged_lines)
                }
                Section::Untracked if entry.staged == b'?' => (FileState::Untracked, None),
                _ => return None,
            };
            Some(SectionFile {
                path: path.clone(),
                state,
                lines,
                original: entry
                    .original
                    .clone()
                    .filter(|_| section == Section::Staged),
            })
        })
        .collect();
    if section == Section::Untracked {
        files.extend(
            snapshot
                .untracked_directories
                .iter()
                .map(|directory| SectionFile {
                    path: directory.clone(),
                    state: FileState::Untracked,
                    lines: None,
                    original: None,
                }),
        );
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

/// A letter of `git status --short` as a state.
fn side_state(letter: u8) -> FileState {
    match letter {
        b'A' | b'C' => FileState::Added,
        b'D' => FileState::Deleted,
        b'R' => FileState::Renamed,
        _ => FileState::Modified,
    }
}

/// What the status buffers want read of each working tree.
pub fn wants(editor: &Editor) -> HashMap<PathBuf, Wants> {
    let mut wants: HashMap<PathBuf, Wants> = HashMap::new();
    for view in editor
        .buffers()
        .iter()
        .filter_map(|buffer| buffer.local::<StatusView>())
    {
        let wanted = wants.entry(view.root.clone()).or_default();
        wanted.details = true;
        wanted.history = Some(view.history.wants());
        let mut diffs: Vec<DiffKey> = view
            .open
            .iter()
            .filter(|(section, _)| !view.folded.contains(section))
            .map(|(section, path)| DiffKey {
                staged: *section == Section::Staged,
                path: path.clone(),
            })
            .collect();
        diffs.sort_by(|a, b| (&a.path, a.staged).cmp(&(&b.path, b.staged)));
        wanted.diffs.extend(diffs);
    }
    wants
}

/// Draws the status buffers whose trees were read again or whose folds changed. Runs
/// after every command.
pub fn sync(editor: &mut Editor) {
    let Some(repositories) = editor.find_plugin_state::<Repositories>() else {
        return;
    };
    let mut stale: Vec<(usize, u64, Snapshot)> = Vec::new();
    for (index, buffer) in editor.buffers().iter().enumerate() {
        let Some(view) = buffer.local::<StatusView>() else {
            continue;
        };
        let Some(repository) = repositories.get(&view.root) else {
            continue;
        };
        if let Some(snapshot) = &repository.snapshot
            && view.drawn != Some(repository.generation)
        {
            stale.push((index, repository.generation, snapshot.clone()));
        }
    }
    for (index, generation, snapshot) in stale {
        draw(&mut editor.buffers_mut()[index], &snapshot, generation);
    }
}

/// Rewrites the buffer's lines from `snapshot`, point staying on what it was on.
fn draw(buffer: &mut Buffer, snapshot: &Snapshot, generation: u64) {
    let current_line = buffer.point_line();
    let Some(view) = buffer.local_mut::<StatusView>() else {
        return;
    };
    let current = view.lines.get(current_line).cloned();
    let first_draw = view.drawn.is_none() && view.lines.is_empty();
    let (lines, texts) = layout(view, snapshot);
    let target = match &current {
        _ if first_draw => lines.iter().position(|line| matches!(line, Line::File(..))),
        Some(current) => lines.iter().position(|line| line == current).or_else(|| {
            let (section, path) = current.file()?;
            lines
                .iter()
                .position(|line| *line == Line::File(section, path.to_path_buf()))
        }),
        None => None,
    }
    .unwrap_or(current_line.min(lines.len().saturating_sub(1)));
    view.lines = lines;
    view.drawn = Some(generation);
    buffer.replace_text(Text::from(texts.join("\n").as_str()));
    let offset = buffer.text().line_start(target).unwrap_or_default();
    buffer.set_point(offset);
}

/// The lines of the buffer and their texts.
fn layout(view: &StatusView, snapshot: &Snapshot) -> (Vec<Line>, Vec<String>) {
    let (mut lines, mut texts) = (Vec::new(), Vec::new());
    for section in Section::FILES {
        let files = section_files(snapshot, section);
        if files.is_empty() {
            continue;
        }
        lines.push(Line::Heading(section));
        texts.push(format!("{} {}", section.title(), files.len()));
        if view.folded.contains(&section) {
            continue;
        }
        for file in files {
            lines.push(Line::File(section, file.path.clone()));
            texts.push(format!("{} {}", file.state.letter(), file.path.display()));
            let open = view.open.contains(&(section, file.path.clone()));
            let key = DiffKey {
                staged: section == Section::Staged,
                path: file.path.clone(),
            };
            if let Some(diff) = snapshot.diffs.get(&key).filter(|_| open) {
                push_diff(&mut lines, &mut texts, section, &file.path, diff);
            }
        }
    }
    if snapshot.stashes > 0 {
        lines.push(Line::Heading(Section::Stashes));
        texts.push(format!("Stashes {}", snapshot.stashes));
        let stashes = snapshot
            .details
            .as_ref()
            .map_or(&[][..], |details| &details.stashes);
        if !view.folded.contains(&Section::Stashes) {
            for (index, stash) in stashes.iter().enumerate() {
                lines.push(Line::Stash(index));
                texts.push(format!("{} {}", stash.name, stash.message));
            }
        }
    }
    (lines, texts)
}

fn push_diff(
    lines: &mut Vec<Line>,
    texts: &mut Vec<String>,
    section: Section,
    path: &Path,
    diff: &FileDiff,
) {
    if diff.hunks.is_empty() {
        lines.push(Line::NoDiff(section, path.to_path_buf()));
        texts.push(no_diff_text(diff).into());
        return;
    }
    let mut shown = 0;
    for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
        let diff_line = |line| Line::Diff {
            section,
            path: path.to_path_buf(),
            hunk: hunk_index,
            line,
        };
        lines.push(diff_line(None));
        texts.push(hunk.header.clone());
        for (index, text) in hunk.lines.iter().enumerate() {
            if shown == MAX_DIFF_LINES {
                return;
            }
            shown += 1;
            lines.push(diff_line(Some(index)));
            texts.push(text.clone());
        }
    }
}

fn no_diff_text(diff: &FileDiff) -> &'static str {
    if diff.binary {
        "Binary file"
    } else {
        "No changes to show"
    }
}

/// The window's status buffer as an outline, if it is one.
pub fn outline(editor: &Editor, range: Range<usize>) -> Option<Outline> {
    let view = editor.window_buffer().local::<StatusView>()?;
    let snapshot = editor
        .find_plugin_state::<Repositories>()
        .and_then(|repositories| repositories.get(&view.root)?.snapshot.as_ref());
    let rows = match snapshot {
        Some(snapshot) => view
            .lines
            .get(range.start.min(view.lines.len())..range.end.min(view.lines.len()))
            .unwrap_or_default()
            .iter()
            .filter_map(|line| row(view, snapshot, line))
            .collect(),
        None => Vec::new(),
    };
    Some(Outline {
        facts: snapshot.map(facts).unwrap_or_default(),
        rows,
        toggle: TOGGLE,
        activate: VISIT,
        hints: HINTS,
        history: snapshot.map(|snapshot| history_view::history(view, snapshot)),
    })
}

fn row(view: &StatusView, snapshot: &Snapshot, line: &Line) -> Option<OutlineRow> {
    Some(match line {
        Line::Heading(section) => OutlineRow::Heading {
            title: section.title().into(),
            count: match section {
                Section::Stashes => snapshot.stashes,
                _ => section_files(snapshot, *section).len(),
            },
            open: !view.folded.contains(section),
            tone: if *section == Section::Unmerged {
                Tone::Danger
            } else {
                Tone::Plain
            },
        },
        Line::File(section, path) => {
            let file = section_files(snapshot, *section)
                .into_iter()
                .find(|file| file.path == *path)?;
            let is_directory = snapshot.untracked_directories.contains(path);
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            OutlineRow::File {
                state: file.state,
                name: if is_directory {
                    format!("{name}/")
                } else {
                    name
                },
                directory: directory_label(path),
                lines: file.lines,
                open: view.open.contains(&(*section, path.clone())),
            }
        }
        Line::Diff {
            section,
            path,
            hunk,
            line,
        } => {
            let key = DiffKey {
                staged: *section == Section::Staged,
                path: path.clone(),
            };
            let hunk = snapshot.diffs.get(&key)?.hunks.get(*hunk)?;
            OutlineRow::Diff(match line {
                None => DiffRow::Hunk(hunk.header.clone()),
                Some(index) => diff_row(hunk.lines.get(*index)?),
            })
        }
        Line::NoDiff(section, path) => {
            let key = DiffKey {
                staged: *section == Section::Staged,
                path: path.clone(),
            };
            OutlineRow::Diff(DiffRow::Context(
                no_diff_text(snapshot.diffs.get(&key)?).into(),
            ))
        }
        Line::Stash(index) => {
            let stash = snapshot.details.as_ref()?.stashes.get(*index)?;
            OutlineRow::Item {
                name: stash.name.clone(),
                description: stash.message.clone(),
            }
        }
    })
}

fn diff_row(text: &str) -> DiffRow {
    let rest = text.get(1..).unwrap_or_default().to_owned();
    match text.as_bytes().first() {
        Some(b'+') => DiffRow::Added(rest),
        Some(b'-') => DiffRow::Removed(rest),
        Some(b' ') => DiffRow::Context(rest),
        _ => DiffRow::Context(text.to_owned()),
    }
}

/// `src/`, or `./` at the root.
pub fn directory_label(path: &Path) -> String {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => format!("{}/", parent.display()),
        _ => "./".into(),
    }
}

fn facts(snapshot: &Snapshot) -> Vec<Fact> {
    let details = snapshot.details.as_ref();
    let subject = details.and_then(|details| details.subject.clone());
    let head = match &snapshot.head {
        Head::Branch(name) => name.clone(),
        Head::Unborn(name) => name.clone(),
        Head::Detached(id) => id.chars().take(SHORT_ID).collect(),
    };
    let mut facts = vec![Fact {
        label: "Head",
        value: FactValue::Ref(RefChip {
            name: head,
            kind: RefKind::Head,
        }),
        note: match &snapshot.head {
            Head::Unborn(_) => "No commits yet".into(),
            _ => subject.unwrap_or_default(),
        },
        note_is_title: true,
        buttons: Vec::new(),
    }];
    if let Some(upstream) = &snapshot.upstream {
        facts.push(Fact {
            label: "Upstream",
            value: FactValue::Code(upstream.name.clone()),
            note: match upstream.ahead_behind {
                None => "Gone from the remote".into(),
                Some((0, 0)) => "Up to date".into(),
                Some((ahead, 0)) => format!("{} to push", commits(ahead)),
                Some((0, behind)) => format!("{} to pull", commits(behind)),
                Some((ahead, behind)) => format!("{ahead} to push · {behind} to pull"),
            },
            note_is_title: false,
            buttons: Vec::new(),
        });
    }
    if let Some(base) = details.and_then(|details| details.base.as_ref()) {
        facts.push(Fact {
            label: "Base",
            value: FactValue::Ref(RefChip {
                name: base.name.clone(),
                kind: RefKind::Main,
            }),
            note: match base.new_commits {
                0 => "Up to date".into(),
                count => format!("{count} new {}", plural(count, "commit")),
            },
            note_is_title: false,
            buttons: vec![
                Button {
                    label: "Rebase".into(),
                    selected: false,
                    action: Rc::new(integrate::rebase),
                },
                Button {
                    label: "Merge".into(),
                    selected: false,
                    action: Rc::new(integrate::merge),
                },
            ],
        });
    }
    facts
}

/// `1 commit`, `2 commits`.
fn commits(count: u64) -> String {
    format!("{count} {}", plural(count, "commit"))
}

fn plural(count: u64, noun: &str) -> String {
    if count == 1 {
        noun.into()
    } else {
        format!("{noun}s")
    }
}

/// The branch checked out, for the pill of a status buffer.
pub fn branch_status(editor: &Editor) -> Option<StatusSegment> {
    let snapshot = window_snapshot(editor)?;
    let name = match &snapshot.head {
        Head::Branch(name) | Head::Unborn(name) => name.clone(),
        Head::Detached(id) => id.chars().take(SHORT_ID).collect(),
    };
    Some(StatusSegment {
        items: vec![StatusItem::new(Some(Icon::Branch), name)],
        command: None,
        active: false,
        code: true,
    })
}

/// Commits ahead of and behind the upstream, for the pill of a status buffer.
pub fn sync_status(editor: &Editor) -> Option<StatusSegment> {
    let (ahead, behind) = window_snapshot(editor)?.upstream.as_ref()?.ahead_behind?;
    Some(StatusSegment {
        items: vec![StatusItem::toned(
            format!("↑{ahead} ↓{behind}"),
            Tone::Faint,
        )],
        command: None,
        active: false,
        code: true,
    })
}

fn window_snapshot(editor: &Editor) -> Option<&Snapshot> {
    let view = editor.window_buffer().local::<StatusView>()?;
    editor
        .find_plugin_state::<Repositories>()?
        .get(&view.root)?
        .snapshot
        .as_ref()
}
