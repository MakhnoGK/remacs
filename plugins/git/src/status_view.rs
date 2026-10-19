//! The status buffer, like Emacs magit-status: facts about the branch, then the changed
//! files by section, each line one row, a file's diff unfolded under it.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_core::{
    Buffer, Button, DiffRow, Editor, Fact, FactValue, FileState, Icon, LineCounts, Outline,
    OutlineRow, RefChip, RefKind, StatusItem, StatusSegment, Tone, ToolbarButton,
};
use remacs_text::Text;

use crate::conflict_view::{self, ConflictState};
use crate::diff::FileDiff;
use crate::history_view::{self, HistoryState};
use crate::integrate;
use crate::monitor::Wants;
use crate::repositories::Repositories;
use crate::settings::GitSettings;
use crate::stash::StashContents;
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
    ("F", "Pull"),
    ("P", "Push"),
    ("z", "Stash"),
    ("s", "Stage"),
    ("u", "Unstage"),
];
/// Diff lines shown of one file at most, so a huge change does not swamp the buffer.
pub const MAX_DIFF_LINES: usize = 2000;
pub const SHORT_ID: usize = 7;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Section {
    Unmerged,
    /// Staged files that had conflicts in the rebase or merge in progress.
    Resolved,
    Staged,
    Unstaged,
    Untracked,
    Stashes,
}

impl Section {
    const FILES: [Section; 5] = [
        Section::Unmerged,
        Section::Resolved,
        Section::Staged,
        Section::Unstaged,
        Section::Untracked,
    ];

    /// While a rebase or merge is in progress, what is staged merged cleanly.
    fn title(self, snapshot: &Snapshot) -> &'static str {
        match self {
            Section::Unmerged => "Unmerged",
            Section::Resolved => "Resolved",
            Section::Staged if snapshot.operation.is_some() => "Merged cleanly",
            Section::Staged => "Staged",
            Section::Unstaged => "Unstaged",
            Section::Untracked => "Untracked",
            Section::Stashes => "Stashes",
        }
    }

    /// Whether its files have a diff to unfold: an untracked file's is all added lines.
    pub fn has_diffs(self) -> bool {
        matches!(
            self,
            Section::Resolved | Section::Staged | Section::Unstaged | Section::Untracked
        )
    }

    /// Its files' changes are in the index.
    pub fn is_staged(self) -> bool {
        matches!(self, Section::Resolved | Section::Staged)
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
    /// After the files an unfolded untracked directory lists, those left out.
    MoreFiles(PathBuf),
    /// By its place in the list.
    Stash(usize),
    /// A file of an unfolded stash, by the stash's id.
    StashFile {
        stash: String,
        path: PathBuf,
    },
    /// A line of a stashed file's diff, as in `Diff`; for one without lines, hunk 0 and no
    /// line.
    StashDiff {
        stash: String,
        path: PathBuf,
        hunk: usize,
        line: Option<usize>,
    },
}

impl Line {
    /// The file the line is, or is part of the diff of.
    pub fn file(&self) -> Option<(Section, &Path)> {
        match self {
            Line::File(section, path)
            | Line::Diff { section, path, .. }
            | Line::NoDiff(section, path) => Some((*section, path)),
            Line::MoreFiles(directory) => Some((Section::Untracked, directory)),
            _ => None,
        }
    }

    /// The stash the line is, by its place in the list.
    pub fn stash(&self) -> Option<usize> {
        match self {
            Line::Stash(index) => Some(*index),
            _ => None,
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
    pub conflicts: ConflictState,
    /// Stashes unfolded, by id.
    open_stashes: HashMap<String, StashContents>,
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
            conflicts: ConflictState::default(),
            open_stashes: HashMap::new(),
        }
    }

    /// Unfolds the stashes and the stash `id`, which holds `contents`.
    pub fn show_stash(&mut self, id: String, contents: StashContents) {
        self.folded.remove(&Section::Stashes);
        self.open_stashes.insert(id, contents);
        self.drawn = None;
    }

    /// Folds the stash `id`; false when it was not unfolded.
    pub fn fold_stash(&mut self, id: &str) -> bool {
        self.drawn = None;
        self.open_stashes.remove(id).is_some()
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

    /// Whether the diff of a file in the buffer is shown.
    pub fn any_file_open(&self) -> bool {
        self.open
            .iter()
            .any(|(section, path)| self.lines.contains(&Line::File(*section, path.clone())))
    }

    /// Unfolds the sections with diffs and shows the diff of each of their files.
    pub fn open_all(&mut self, snapshot: &Snapshot) {
        for section in Section::FILES
            .into_iter()
            .filter(|section| section.has_diffs())
        {
            self.folded.remove(&section);
            self.open.extend(
                section_files(snapshot, section)
                    .into_iter()
                    .map(|file| (section, file.path)),
            );
        }
        self.drawn = None;
    }

    pub fn close_all(&mut self) {
        self.open.clear();
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
            let staged = !matches!(entry.staged, b'.' | b'?');
            let resolved = snapshot.resolved.contains(path);
            let (state, lines) = match section {
                Section::Unmerged if entry.conflicted => (FileState::Conflicted, None),
                _ if entry.conflicted => return None,
                Section::Resolved if staged && resolved => {
                    (FileState::Resolved, entry.staged_lines)
                }
                Section::Staged if staged && !resolved => {
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
                original: entry.original.clone().filter(|_| section.is_staged()),
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

/// A file of `section`, or one an unfolded untracked directory lists.
fn section_file(snapshot: &Snapshot, section: Section, path: &Path) -> Option<SectionFile> {
    let listed = || {
        snapshot
            .listings
            .values()
            .any(|listing| listing.files.iter().any(|file| file == path))
    };
    section_files(snapshot, section)
        .into_iter()
        .find(|file| file.path == path)
        .or_else(|| {
            (section == Section::Untracked && listed()).then(|| SectionFile {
                path: path.to_path_buf(),
                state: FileState::Untracked,
                lines: None,
                original: None,
            })
        })
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
    let settings = editor
        .find_plugin_state::<GitSettings>()
        .cloned()
        .unwrap_or_default();
    for view in editor
        .buffers()
        .iter()
        .filter_map(|buffer| buffer.local::<StatusView>())
    {
        let wanted = wants.entry(view.root.clone()).or_default();
        wanted.details = true;
        wanted.settings = settings.clone();
        wanted.history = Some(view.history.wants());
        let mut diffs: Vec<DiffKey> = view
            .open
            .iter()
            .filter(|(section, _)| !view.folded.contains(section))
            .map(|(section, path)| DiffKey {
                staged: section.is_staged(),
                path: path.clone(),
            })
            .collect();
        diffs.sort_by(|a, b| (&a.path, a.staged).cmp(&(&b.path, b.staged)));
        wanted.diffs.extend(diffs);
    }
    wants
}

/// Draws the window's status buffer if its tree was read again or its folds changed;
/// a hidden one waits until it is shown. Runs after every command.
pub fn sync(editor: &mut Editor) {
    let Some(view) = editor.window_buffer().local::<StatusView>() else {
        return;
    };
    let Some(repository) = editor
        .find_plugin_state::<Repositories>()
        .and_then(|repositories| repositories.get(&view.root))
    else {
        return;
    };
    if let Some(snapshot) = repository.snapshot.clone()
        && view.drawn != Some(repository.generation)
    {
        let generation = repository.generation;
        draw(editor.window_buffer_mut(), &snapshot, generation);
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
    if let Some(details) = &snapshot.details {
        view.open_stashes
            .retain(|id, _| details.stashes.iter().any(|stash| stash.id == *id));
    }
    if snapshot.operation.is_none() {
        view.conflicts = ConflictState::default();
    }
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
        texts.push(format!("{} {}", section.title(snapshot), files.len()));
        if view.folded.contains(&section) {
            continue;
        }
        for file in files {
            push_file(&mut lines, &mut texts, view, snapshot, section, &file);
            let listing = snapshot
                .listings
                .get(&file.path)
                .filter(|_| view.open.contains(&(section, file.path.clone())));
            if let Some(listing) = listing {
                for path in &listing.files {
                    let listed = SectionFile {
                        path: path.clone(),
                        state: FileState::Untracked,
                        lines: None,
                        original: None,
                    };
                    push_file(&mut lines, &mut texts, view, snapshot, section, &listed);
                }
                if listing.more > 0 {
                    lines.push(Line::MoreFiles(file.path.clone()));
                    texts.push(format!("{} more files", listing.more));
                }
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
                if let Some(contents) = view.open_stashes.get(&stash.id) {
                    push_stash(&mut lines, &mut texts, &stash.id, contents);
                }
            }
        }
    }
    (lines, texts)
}

/// A file's line and, unfolded, its diff.
fn push_file(
    lines: &mut Vec<Line>,
    texts: &mut Vec<String>,
    view: &StatusView,
    snapshot: &Snapshot,
    section: Section,
    file: &SectionFile,
) {
    let path = &file.path;
    lines.push(Line::File(section, path.clone()));
    texts.push(format!("{} {}", file.state.letter(), path.display()));
    let key = DiffKey {
        staged: section.is_staged(),
        path: path.clone(),
    };
    let open = view.open.contains(&(section, path.clone()));
    if let Some(diff) = snapshot.diffs.get(&key).filter(|_| open) {
        push_diff(
            lines,
            texts,
            Some(diff),
            Line::NoDiff(section, path.clone()),
            |hunk, line| Line::Diff {
                section,
                path: path.clone(),
                hunk,
                line,
            },
        );
    }
}

/// An unfolded stash's files, each with its diff.
fn push_stash(
    lines: &mut Vec<Line>,
    texts: &mut Vec<String>,
    stash: &str,
    contents: &StashContents,
) {
    for file in &contents.files {
        let path = &file.path;
        lines.push(Line::StashFile {
            stash: stash.to_owned(),
            path: path.clone(),
        });
        texts.push(format!("{} {}", file.state.letter(), path.display()));
        let diff_line = |hunk, line| Line::StashDiff {
            stash: stash.to_owned(),
            path: path.clone(),
            hunk,
            line,
        };
        push_diff(
            lines,
            texts,
            contents.diffs.get(path),
            diff_line(0, None),
            diff_line,
        );
    }
}

/// A diff's hunks, `diff_line` making the line of a hunk's header or of one of its lines;
/// `empty` in place of a diff without lines.
pub fn push_diff<L>(
    lines: &mut Vec<L>,
    texts: &mut Vec<String>,
    diff: Option<&FileDiff>,
    empty: L,
    diff_line: impl Fn(usize, Option<usize>) -> L,
) {
    let Some(diff) = diff.filter(|diff| !diff.hunks.is_empty()) else {
        lines.push(empty);
        texts.push(no_diff_text(diff));
        return;
    };
    let mut shown = 0;
    for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
        lines.push(diff_line(hunk_index, None));
        texts.push(hunk.header.clone());
        for (index, text) in hunk.lines.iter().enumerate() {
            if shown == MAX_DIFF_LINES {
                return;
            }
            shown += 1;
            lines.push(diff_line(hunk_index, Some(index)));
            texts.push(text.clone());
        }
    }
}

fn no_diff_text(diff: Option<&FileDiff>) -> String {
    diff.map_or_else(|| "No changes to show".into(), FileDiff::summary)
}

/// The window's status buffer as an outline, if it is one.
pub fn outline(editor: &Editor, range: Range<usize>) -> Option<Outline> {
    let view = editor.window_buffer().local::<StatusView>()?;
    let snapshot = editor
        .find_plugin_state::<Repositories>()
        .and_then(|repositories| repositories.get(&view.root)?.snapshot.as_deref());
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
    let point_line = editor.window_buffer().point_line();
    let conflict = snapshot.and_then(|snapshot| {
        let (Section::Unmerged, path) = view.line(point_line)?.file()? else {
            return None;
        };
        conflict_view::view(editor, view, snapshot, path)
    });
    let in_conflict = conflict.is_some();
    Some(Outline {
        facts: snapshot.map(facts).unwrap_or_default(),
        rows,
        toggle: TOGGLE,
        activate: VISIT,
        hints: match snapshot.and_then(|snapshot| snapshot.operation.as_ref()) {
            _ if in_conflict => conflict_view::hints(snapshot),
            Some(_) => conflict_view::OPERATION_HINTS,
            None => HINTS,
        },
        history: snapshot
            .filter(|_| !in_conflict)
            .map(|snapshot| history_view::history(view, snapshot)),
        conflict,
        banner: snapshot.and_then(conflict_view::banner),
        toolbar: snapshot.map(toolbar).unwrap_or_default(),
    })
}

/// Pull and Push, with the commits each would move; off while something is in progress
/// or off a branch.
fn toolbar(snapshot: &Snapshot) -> Vec<ToolbarButton> {
    let ready = snapshot.operation.is_none() && matches!(snapshot.head, Head::Branch(_));
    let (ahead, behind) = snapshot
        .upstream
        .as_ref()
        .and_then(|upstream| upstream.ahead_behind)
        .unwrap_or_default();
    let count = |count: u64| (count > 0).then_some(count as usize);
    vec![
        ToolbarButton {
            icon: Icon::Pull,
            label: "Pull".into(),
            count: count(behind),
            enabled: ready && snapshot.upstream.is_some(),
            action: Rc::new(crate::sync::pull),
        },
        ToolbarButton {
            icon: Icon::Push,
            label: "Push".into(),
            count: count(ahead),
            enabled: ready,
            action: Rc::new(crate::sync::push),
        },
    ]
}

fn row(view: &StatusView, snapshot: &Snapshot, line: &Line) -> Option<OutlineRow> {
    Some(match line {
        Line::Heading(section) => OutlineRow::Heading {
            title: section.title(snapshot).into(),
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
            let file = section_file(snapshot, *section, path)?;
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
                note: conflict_view::file_note(view, snapshot, *section, path),
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
                staged: section.is_staged(),
                path: path.clone(),
            };
            hunk_row(snapshot.diffs.get(&key)?, *hunk, *line)?
        }
        Line::NoDiff(section, path) => {
            let key = DiffKey {
                staged: section.is_staged(),
                path: path.clone(),
            };
            OutlineRow::Diff(DiffRow::Context(no_diff_text(Some(
                snapshot.diffs.get(&key)?,
            ))))
        }
        Line::MoreFiles(directory) => OutlineRow::Item {
            name: "…".into(),
            description: format!("{} more files", snapshot.listings.get(directory)?.more),
        },
        Line::Stash(index) => {
            let stash = snapshot.details.as_ref()?.stashes.get(*index)?;
            OutlineRow::Item {
                name: stash.name.clone(),
                description: stash.message.clone(),
            }
        }
        Line::StashFile { stash, path } => {
            let file = view
                .open_stashes
                .get(stash)?
                .files
                .iter()
                .find(|file| file.path == *path)?;
            OutlineRow::File {
                state: file.state,
                name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                directory: directory_label(path),
                lines: file.lines,
                note: None,
                open: true,
            }
        }
        Line::StashDiff {
            stash,
            path,
            hunk,
            line,
        } => {
            let diff = view.open_stashes.get(stash)?.diffs.get(path);
            match diff.filter(|diff| !diff.hunks.is_empty()) {
                Some(diff) => hunk_row(diff, *hunk, *line)?,
                None => OutlineRow::Diff(DiffRow::Context(no_diff_text(diff))),
            }
        }
    })
}

/// A hunk's header, or one of its lines.
pub fn hunk_row(diff: &FileDiff, hunk: usize, line: Option<usize>) -> Option<OutlineRow> {
    let hunk = diff.hunks.get(hunk)?;
    Some(OutlineRow::Diff(match line {
        None => DiffRow::Hunk(hunk.header.clone()),
        Some(index) => diff_row(hunk.lines.get(index)?),
    }))
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
    if let Some(operation) = &snapshot.operation {
        return conflict_view::facts(operation);
    }
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
    if let Some(operation) = &snapshot.operation {
        return Some(StatusSegment {
            items: vec![StatusItem::toned(
                conflict_view::progress(operation),
                Tone::Warning,
            )],
            command: None,
            active: false,
            code: false,
        });
    }
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
    let snapshot = window_snapshot(editor)?;
    if snapshot.operation.is_some() {
        let item = match conflict_view::conflicts_left(snapshot) {
            0 => StatusItem::toned("Resolved".to_owned(), Tone::Success),
            1 => StatusItem::toned("1 conflict".to_owned(), Tone::Danger),
            left => StatusItem::toned(format!("{left} conflicts"), Tone::Danger),
        };
        return Some(StatusSegment {
            items: vec![item],
            command: None,
            active: false,
            code: false,
        });
    }
    let (ahead, behind) = snapshot.upstream.as_ref()?.ahead_behind?;
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
        .as_deref()
}
