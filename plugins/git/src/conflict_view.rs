//! A rebase or merge in progress as the status shows it: a banner across the top, facts
//! about what is applied onto what, and a file's conflicts beside the pane.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_core::{
    Banner, BannerButton, BannerPart, ConflictBlock, ConflictRegion, ConflictSide, ConflictView,
    Editor, Fact, FactValue, RefChip, RefKind, Tone,
};

use crate::conflict::{Conflict, Keep, Region};
use crate::operation::{Operation, OperationKind};
use crate::resolve;
use crate::status::{Head, Snapshot};
use crate::status_view::{Section, StatusView, directory_label};

/// Lines shown before the current conflict, and in all at most.
const CONTEXT_LINES: usize = 8;
const MAX_LINES: usize = 120;

pub const OPERATION_HINTS: &[(&str, &str)] =
    &[("C-c C-c", "Continue"), ("C-c C-k", "Abort"), ("s", "Stage"), ("u", "Unstage")];
const REBASE_CONFLICT_HINTS: &[(&str, &str)] = &[
    ("n p", "Next / previous"),
    ("o", "Upstream"),
    ("t", "Yours"),
    ("b", "Both"),
    ("e", "Edit"),
    ("C-c C-c", "Continue"),
    ("C-c C-k", "Abort"),
];
const MERGE_CONFLICT_HINTS: &[(&str, &str)] = &[
    ("n p", "Next / previous"),
    ("o", "Yours"),
    ("t", "Theirs"),
    ("b", "Both"),
    ("e", "Edit"),
    ("C-c C-c", "Continue"),
    ("C-c C-k", "Abort"),
];

/// How a status buffer's conflicts were resolved, and which one each file shows open.
#[derive(Default)]
pub struct ConflictState {
    current: HashMap<PathBuf, usize>,
    resolutions: HashMap<PathBuf, Vec<Resolution>>,
}

/// Lines a conflict was resolved into, from 0, and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub start: usize,
    pub len: usize,
    pub said: &'static str,
}

impl ConflictState {
    /// The conflict open in `path`, of `count` left.
    pub fn current(&self, path: &Path, count: usize) -> usize {
        self.current.get(path).copied().unwrap_or_default().min(count.saturating_sub(1))
    }

    pub fn set_current(&mut self, path: &Path, index: usize) {
        self.current.insert(path.to_path_buf(), index);
    }

    /// Notes that `replaced` lines from `start` became `kept` lines, moving those below.
    pub fn record(
        &mut self,
        path: &Path,
        start: usize,
        replaced: usize,
        kept: usize,
        said: &'static str,
    ) {
        let resolutions = self.resolutions.entry(path.to_path_buf()).or_default();
        for resolution in resolutions.iter_mut().filter(|resolution| resolution.start > start) {
            resolution.start = (resolution.start + kept).saturating_sub(replaced);
        }
        resolutions.push(Resolution { start, len: kept, said });
    }

    pub fn resolutions(&self, path: &Path) -> &[Resolution] {
        self.resolutions.get(path).map_or(&[], Vec::as_slice)
    }
}

pub fn is_rebase(snapshot: &Snapshot) -> bool {
    matches!(snapshot.operation, Some(Operation { kind: OperationKind::Rebase { .. }, .. }))
}

/// Conflicts left in all files; one whose markers were not read counts once.
pub fn conflicts_left(snapshot: &Snapshot) -> usize {
    snapshot
        .files
        .iter()
        .filter(|(_, entry)| entry.conflicted)
        .map(|(path, _)| snapshot.conflicts.get(path).map_or(1, |file| file.count()))
        .sum()
}

fn unmerged_files(snapshot: &Snapshot) -> usize {
    snapshot.files.values().filter(|entry| entry.conflicted).count()
}

pub fn hints(snapshot: Option<&Snapshot>) -> &'static [(&'static str, &'static str)] {
    if snapshot.is_some_and(is_rebase) { REBASE_CONFLICT_HINTS } else { MERGE_CONFLICT_HINTS }
}

fn chip(name: &str, kind: RefKind) -> RefChip {
    RefChip { name: name.into(), kind }
}

/// For the pill: `Rebase 2/3`, `Merging`.
pub fn progress(operation: &Operation) -> String {
    match &operation.kind {
        OperationKind::Rebase { step, total, .. } => format!("Rebase {step}/{total}"),
        OperationKind::Merge => "Merging".into(),
    }
}

/// What the rebase or merge applies onto what.
pub fn facts(operation: &Operation) -> Vec<Fact> {
    let branch = operation.branch.as_deref().unwrap_or("HEAD");
    let other = &operation.other;
    let fact = |label, value, note: String, note_is_title| Fact {
        label,
        value,
        note,
        note_is_title,
        buttons: Vec::new(),
    };
    let mut facts = Vec::new();
    match &operation.kind {
        OperationKind::Rebase { applying, .. } => {
            facts.push(fact(
                "Head",
                FactValue::Ref(chip(branch, RefKind::Head)),
                "rebase in progress".into(),
                false,
            ));
            facts.push(fact(
                "Onto",
                FactValue::Ref(chip(other.label(), RefKind::Main)),
                format!("{} {}", other.id, other.subject),
                false,
            ));
            if let Some(applying) = applying {
                facts.push(fact(
                    "Applying",
                    FactValue::Code(applying.id.clone()),
                    applying.subject.clone(),
                    true,
                ));
            }
        }
        OperationKind::Merge => {
            facts.push(fact(
                "Head",
                FactValue::Ref(chip(branch, RefKind::Head)),
                "merge in progress".into(),
                false,
            ));
            facts.push(fact(
                "Merging",
                FactValue::Ref(chip(other.label(), RefKind::Branch)),
                format!("{} {}", other.id, other.subject),
                false,
            ));
        }
    }
    facts
}

/// Across the top: what is in progress, how far it got and the buttons to go on.
pub fn banner(snapshot: &Snapshot) -> Option<Banner> {
    let operation = snapshot.operation.as_ref()?;
    let branch = operation.branch.as_deref().unwrap_or("HEAD");
    let other = operation.other.label();
    let left = conflicts_left(snapshot);
    let unmerged = unmerged_files(snapshot);
    let state = match (left, unmerged) {
        (0, 0) => ("All conflicts resolved".to_owned(), Tone::Success),
        (0, files) => (format!("{} to stage", plural(files, "file")), Tone::Warning),
        (count, _) => (format!("{} left", plural(count, "conflict")), Tone::Danger),
    };
    let button =
        |label: &str, tone, enabled, action: fn(&mut Editor) -> remacs_core::CommandResult| {
            BannerButton { label: label.into(), tone, enabled, action: Rc::new(action) }
        };
    let ready = unmerged == 0;
    let continue_tone = if ready { Tone::Accent } else { Tone::Plain };
    let (title, parts, step, commit, mut buttons) = match &operation.kind {
        OperationKind::Rebase { step, total, applying, .. } => (
            "Rebasing",
            vec![
                BannerPart::Chip(chip(branch, RefKind::Head)),
                BannerPart::Word("onto".into()),
                BannerPart::Chip(chip(other, RefKind::Main)),
            ],
            (*total > 0).then(|| format!("Step {step} of {total}")),
            applying.as_ref().map(|applying| (applying.id.clone(), applying.subject.clone())),
            vec![button("Skip commit", Tone::Plain, true, resolve::skip)],
        ),
        OperationKind::Merge => (
            "Merging",
            vec![
                BannerPart::Chip(chip(other, RefKind::Branch)),
                BannerPart::Word("into".into()),
                BannerPart::Chip(chip(branch, RefKind::Head)),
            ],
            None,
            Some((operation.other.id.clone(), operation.other.subject.clone())),
            Vec::new(),
        ),
    };
    buttons.push(button("Abort", Tone::Danger, true, resolve::abort));
    buttons.push(button("Continue", continue_tone, ready, resolve::continue_operation));
    Some(Banner { title: title.into(), parts, step, commit, state, buttons })
}

/// Beside an unmerged file, how many conflicts are left in it; beside a resolved one, how
/// many were resolved here.
pub fn file_note(
    view: &StatusView,
    snapshot: &Snapshot,
    section: Section,
    path: &Path,
) -> Option<(String, Tone)> {
    let resolved = view.conflicts.resolutions(path).len();
    match section {
        Section::Unmerged => {
            let left = snapshot.conflicts.get(path)?.count();
            Some(match (left, resolved) {
                (0, _) => ("resolved, to stage".into(), Tone::Warning),
                (left, 0) => (plural(left, "conflict"), Tone::Danger),
                (left, resolved) => (format!("{left} of {} left", left + resolved), Tone::Danger),
            })
        }
        Section::Resolved if resolved > 0 => {
            Some((format!("{resolved} of {resolved}"), Tone::Success))
        }
        _ => None,
    }
}

/// The conflicts of `path` around the open one.
pub fn view(
    editor: &Editor,
    view: &StatusView,
    snapshot: &Snapshot,
    path: &Path,
) -> Option<ConflictView> {
    let file = snapshot.conflicts.get(path)?;
    let left = file.count();
    if left == 0 {
        return None;
    }
    let current = view.conflicts.current(path, left);
    let resolutions = view.conflicts.resolutions(path);
    let starts = file.starts();
    let current_region = file
        .regions
        .iter()
        .enumerate()
        .filter(|(_, region)| matches!(region, Region::Conflict(_)))
        .nth(current)?
        .0;
    let (ours, theirs) = sides(snapshot);
    let mut regions = Vec::new();
    let mut shown = 0;
    let mut conflict_index = file
        .regions
        .iter()
        .take(current_region.saturating_sub(1))
        .filter(|region| matches!(region, Region::Conflict(_)))
        .count();
    for (position, region) in file.regions.iter().enumerate().skip(current_region.saturating_sub(1))
    {
        if shown >= MAX_LINES {
            break;
        }
        let start = starts[position];
        match region {
            Region::Text(lines) => {
                let skip = if position + 1 == current_region {
                    lines.len().saturating_sub(CONTEXT_LINES)
                } else {
                    0
                };
                // The file's last line break leaves no line after it.
                let at_end = position + 1 == file.regions.len()
                    && lines.last().is_some_and(String::is_empty);
                let end = lines.len() - usize::from(at_end);
                let lines = &lines[skip.min(end)..end.min(skip + MAX_LINES - shown)];
                shown += lines.len();
                regions.extend(text_regions(start + skip, lines, resolutions));
            }
            Region::Conflict(conflict) => {
                shown += conflict.ours.len() + conflict.theirs.len();
                regions.push(ConflictRegion::Conflict(Box::new(block(
                    path,
                    conflict,
                    conflict_index,
                    conflict_index == current,
                    start + 1,
                    (&ours, &theirs),
                ))));
                conflict_index += 1;
            }
        }
    }
    let previous: remacs_core::Action = Rc::new(resolve::previous_conflict);
    let next: remacs_core::Action = Rc::new(resolve::next_conflict);
    Some(ConflictView {
        name: path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
        directory: directory_label(path),
        mode: Some(editor.mode_for(Some(&view.root.join(path)))),
        resolved: resolutions.len(),
        current: current + 1,
        left,
        regions,
        previous,
        next,
    })
}

/// `lines` from line `start` (from 0), cut where conflicts were resolved into them.
fn text_regions(start: usize, lines: &[String], resolutions: &[Resolution]) -> Vec<ConflictRegion> {
    let said_at = |line: usize| {
        resolutions
            .iter()
            .find(|resolution| {
                (resolution.start..resolution.start + resolution.len).contains(&line)
            })
            .map(|resolution| (resolution.start, resolution.said))
    };
    let mut regions: Vec<ConflictRegion> = Vec::new();
    let mut run_start = 0;
    for index in 0..=lines.len() {
        let ends = index == lines.len()
            || (index > run_start && said_at(start + index) != said_at(start + run_start));
        if ends && index > run_start {
            regions.push(ConflictRegion::Text {
                first: start + run_start + 1,
                lines: lines[run_start..index].to_vec(),
                resolved: said_at(start + run_start).map(|(_, said)| format!("Resolved · {said}")),
            });
            run_start = index;
        }
    }
    regions
}

/// The upstream and the commit replayed when rebasing; yours and theirs when merging.
fn sides(snapshot: &Snapshot) -> (SideLabel, SideLabel) {
    let Some(operation) = &snapshot.operation else {
        return (
            SideLabel::new("HEAD", RefKind::Head, "yours", None, String::new()),
            SideLabel::new("theirs", RefKind::Branch, "theirs", None, String::new()),
        );
    };
    let other = &operation.other;
    match &operation.kind {
        OperationKind::Rebase { applying, .. } => {
            let applying = applying.clone().unwrap_or_default();
            (
                SideLabel::new(
                    other.label(),
                    RefKind::Main,
                    "upstream",
                    Some(other.id.clone()),
                    other.subject.clone(),
                ),
                SideLabel::new(&applying.id, RefKind::Head, "your commit", None, applying.subject),
            )
        }
        OperationKind::Merge => {
            let head = match &snapshot.head {
                Head::Branch(name) => name.clone(),
                _ => "HEAD".into(),
            };
            let subject = snapshot
                .details
                .as_ref()
                .and_then(|details| details.subject.clone())
                .unwrap_or_default();
            (
                SideLabel::new(&head, RefKind::Head, "yours", None, subject),
                SideLabel::new(
                    other.label(),
                    RefKind::Branch,
                    "theirs",
                    Some(other.id.clone()),
                    other.subject.clone(),
                ),
            )
        }
    }
}

#[derive(Clone)]
struct SideLabel {
    chip: RefChip,
    role: &'static str,
    id: Option<String>,
    subject: String,
}

impl SideLabel {
    fn new(
        name: &str,
        kind: RefKind,
        role: &'static str,
        id: Option<String>,
        subject: String,
    ) -> Self {
        Self { chip: chip(name, kind), role, id, subject }
    }

    fn side(&self, first: usize, lines: &[String]) -> ConflictSide {
        ConflictSide {
            chip: self.chip.clone(),
            role: self.role.into(),
            id: self.id.clone(),
            subject: self.subject.clone(),
            first,
            lines: lines.to_vec(),
        }
    }
}

fn block(
    path: &Path,
    conflict: &Conflict,
    index: usize,
    current: bool,
    first: usize,
    (ours, theirs): (&SideLabel, &SideLabel),
) -> ConflictBlock {
    let keep = |keep: Keep| -> remacs_core::Action {
        let path = path.to_path_buf();
        Rc::new(move |editor| resolve::keep_at(editor, &path, index, keep))
    };
    let path = path.to_path_buf();
    ConflictBlock {
        current,
        ours: ours.side(first, &conflict.ours),
        theirs: theirs.side(first, &conflict.theirs),
        keep_ours: keep(Keep::Ours),
        keep_theirs: keep(Keep::Theirs),
        keep_both: keep(Keep::Both),
        edit: Rc::new(move |editor| resolve::edit_at(editor, &path, index)),
    }
}

/// `1 conflict`, `2 files`.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}
