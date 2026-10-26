//! What one commit changed, like Emacs magit-revision: who made it and when, its message
//! and its files in a column at the left, and the diff of all its files as text, so every
//! motion, search and copy works on it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::Write;
use std::io;
use std::ops::Range;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use remacs_core::{
    Action, Buffer, Button, CodeHighlighter, CommandResult, ConsoleColor, ConsoleStyle, DiffRow,
    Editor, Face, Fact, FactValue, FileState, LineExtra, OutlineRow, Span, TextControls, TextSide,
    Tone,
};
use remacs_text::Text;

use crate::command::{git, output, read_output};
use crate::diff::{FileDiff, read_patch, size};
use crate::history_view::ago;
use crate::reads;
use crate::settings::GitSettings;
use crate::status_view::{MAX_DIFF_LINES, SHORT_ID, directory_label, push_diff};

pub const COMMIT_MODE: &str = "Git Commit";
pub const TOGGLE: &str = "git-commit-toggle";
pub const VISIT: &str = "git-commit-visit";
const NOT_A_COMMIT_BUFFER: &str = "Not a commit buffer";
const HINTS: &[(&str, &str)] = &[
    ("n/p", "Hunk"),
    ("N/P", "File"),
    ("↵", "Visit file"),
    ("o", "File at commit"),
    ("TAB", "Message"),
    ("q", "Back"),
];
/// Its fields end in NULs, the last before the patch.
const FORMAT: &str = "--format=%H%x00%P%x00%an%x00%at%x00%ad%x00%B%x00";
const FIELDS: usize = 6;
const DATE: &str = "--date=format:%Y-%m-%d %H:%M";
/// Of the theme's 16, for a removed line's `-` and an added line's `+`.
const REMOVED_SIGN: u8 = 1;
const ADDED_SIGN: u8 = 2;

/// A commit as `git show` tells it, its changes against its first parent.
pub struct Commit {
    pub id: String,
    /// With their subjects.
    pub parents: Vec<(String, String)>,
    pub author: String,
    /// Seconds since the epoch.
    pub time: i64,
    /// As the author's clock read, e.g. `2026-10-14 18:45`.
    pub date: String,
    pub subject: String,
    /// The message after the subject, blank lines around it left out.
    pub body: Vec<String>,
    pub files: Vec<(PathBuf, FileDiff)>,
}

/// What a line of the commit buffer stands for; files by their place in the commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitLine {
    /// A file's header: its state, path and lines changed.
    File(usize),
    /// A hunk's header, or with `line`, a line of it.
    Diff {
        file: usize,
        hunk: usize,
        line: Option<usize>,
    },
    /// In place of the hunks of a file that has none to show.
    NoDiff(usize),
}

impl CommitLine {
    fn file(self) -> usize {
        match self {
            CommitLine::File(file) | CommitLine::Diff { file, .. } | CommitLine::NoDiff(file) => {
                file
            }
        }
    }
}

/// Faces of a file's hunk lines past their `+`, `-` or ` `, by hunk and line.
type FileFaces = Vec<Vec<Vec<Span>>>;

/// The state of a commit buffer.
pub struct CommitView {
    pub root: PathBuf,
    pub commit: Commit,
    message_folded: bool,
    side_rows: SideRows,
    pub lines: Vec<CommitLine>,
    /// Each line's numbers in the old and the new file, where it has them.
    numbers: Vec<(Option<usize>, Option<usize>)>,
    /// Of the largest number.
    digits: usize,
    /// To highlight each file's code in its mode, as it is first drawn.
    highlighters: Vec<CodeHighlighter>,
    modes: Vec<&'static str>,
    faces: RefCell<HashMap<usize, Rc<FileFaces>>>,
    /// The part of a line that changed against its pair, by line, in bytes of the line.
    changed: HashMap<usize, Range<usize>>,
}

impl CommitView {
    fn faces_of(&self, file: usize) -> Rc<FileFaces> {
        let mut faces = self.faces.borrow_mut();
        let faces = faces.entry(file).or_insert_with(|| {
            let diff = &self.commit.files[file].1;
            Rc::new(highlight_file(&self.highlighters, self.modes[file], diff))
        });
        Rc::clone(faces)
    }
}

/// Shows commit `id` of the working tree at `root` in a buffer of its own, scrolled to
/// `file` if it is one of the commit's; read in the background, a large one taking
/// seconds.
pub fn open(editor: &mut Editor, root: &Path, id: &str, file: Option<&Path>) -> CommandResult {
    if switch_to_shown(editor, root, id) {
        return Ok(());
    }
    let max_bytes = editor.plugin_state::<GitSettings>().commit_max_bytes;
    let (root, id) = (root.to_path_buf(), id.to_owned());
    let file = file.map(Path::to_path_buf);
    let read_root = root.clone();
    reads::spawn(
        editor,
        format!("Reading commit {}…", short(&id)),
        move || read(&read_root, &id, max_bytes),
        move |editor, commit| {
            show(editor, &root, commit, file.as_deref());
            Ok(())
        },
    );
    Ok(())
}

/// Switches to the buffer showing commit `id` of `root`, if there is one.
fn switch_to_shown(editor: &mut Editor, root: &Path, id: &str) -> bool {
    let shown = editor
        .buffers()
        .iter()
        .find(|buffer| {
            buffer
                .local::<CommitView>()
                .is_some_and(|view| view.root == root && view.commit.id == id)
        })
        .map(|buffer| buffer.name().to_owned());
    shown.is_some_and(|name| editor.switch_to_buffer(&name))
}

fn show(editor: &mut Editor, root: &Path, commit: Commit, file: Option<&Path>) {
    // Opened again while it was read.
    if switch_to_shown(editor, root, &commit.id) {
        return;
    }
    let (lines, numbers, text) = layout(&commit);
    let changed = changed_parts(&commit, &lines, &numbers);
    let focus = file
        .and_then(|file| commit.files.iter().position(|(path, _)| path == file))
        .and_then(|file| {
            lines
                .iter()
                .position(|line| *line == CommitLine::File(file))
        });
    let largest = numbers.iter().flat_map(|&(old, new)| [old, new]).flatten();
    let digits = largest.max().unwrap_or(0).to_string().len();
    let modes = commit
        .files
        .iter()
        .map(|(path, _)| editor.mode_for(Some(&root.join(path))))
        .collect();
    let name = format!("Commit {}", short(&commit.id));
    let mut buffer = Buffer::new(name, Text::from(text.as_str()), COMMIT_MODE)
        .with_directory(root.to_path_buf())
        .read_only();
    buffer.set_local(CommitView {
        root: root.to_path_buf(),
        side_rows: side_rows(&commit, false),
        commit,
        message_folded: false,
        lines,
        numbers,
        digits,
        highlighters: editor.code_highlighters(),
        modes,
        faces: RefCell::default(),
        changed,
    });
    editor.add_buffer(buffer);
    if let Some(line) = focus {
        go_to(editor, line);
    }
}

/// The commit, its files' diffs kept up to `max_bytes`.
fn read(root: &Path, id: &str, max_bytes: u64) -> Result<Commit, String> {
    let (fields, files) = read_output(
        git(root)
            .args(["-c", "core.quotePath=false", "show", FORMAT, DATE])
            .args(["--patch", "-M", "--diff-merges=first-parent"])
            .args(["--no-color", "--no-ext-diff", "--no-textconv", id, "--"]),
        |printed| -> io::Result<_> {
            let mut fields = Vec::with_capacity(FIELDS);
            while fields.len() < FIELDS {
                let mut field = Vec::new();
                printed.read_until(b'\0', &mut field)?;
                if field.pop() != Some(b'\0') {
                    break;
                }
                fields.push(String::from_utf8_lossy(&field).into_owned());
            }
            Ok((fields, read_patch(printed, max_bytes)?))
        },
    )
    .and_then(|read| read)
    .map_err(|error| format!("git show: {error}"))?;
    let [id, parents, author, time, date, message] = &fields[..] else {
        return Err("git show printed no commit".into());
    };
    let mut message = message.trim().lines();
    let subject = message.next().unwrap_or_default().to_owned();
    let body: Vec<String> = message.map(str::to_owned).collect();
    let body_start = body.iter().position(|line| !line.trim().is_empty());
    let body = body_start.map_or_else(Vec::new, |start| body[start..].to_vec());
    let parents: Vec<&str> = parents.split_whitespace().collect();
    Ok(Commit {
        id: id.to_owned(),
        parents: subjects(root, &parents),
        author: author.to_owned(),
        time: time.parse().unwrap_or_default(),
        date: date.to_owned(),
        subject,
        body,
        files,
    })
}

/// Each commit with its subject.
fn subjects(root: &Path, ids: &[&str]) -> Vec<(String, String)> {
    if ids.is_empty() {
        return Vec::new();
    }
    let printed = output(
        git(root)
            .args(["show", "-s", "--format=%H%x00%s"])
            .args(ids)
            .arg("--"),
    )
    .unwrap_or_default();
    let known: HashMap<String, String> = String::from_utf8_lossy(&printed)
        .lines()
        .filter_map(|line| {
            let (id, subject) = line.split_once('\0')?;
            Some((id.to_owned(), subject.to_owned()))
        })
        .collect();
    ids.iter()
        .map(|id| {
            (
                (*id).to_owned(),
                known.get(*id).cloned().unwrap_or_default(),
            )
        })
        .collect()
}

type Numbers = Vec<(Option<usize>, Option<usize>)>;

/// The buffer's text, each file's header then its hunks; what each line is, and its
/// numbers in the old and the new file.
fn layout(commit: &Commit) -> (Vec<CommitLine>, Numbers, String) {
    let (mut lines, mut texts) = (Vec::new(), Vec::new());
    for (index, (path, diff)) in commit.files.iter().enumerate() {
        lines.push(CommitLine::File(index));
        texts.push(file_header(path, diff));
        match diff.too_large {
            Some(bytes) => {
                lines.push(CommitLine::NoDiff(index));
                texts.push(format!(
                    "Diff of {} not shown, past git.toml commit-max-bytes",
                    size(bytes)
                ));
            }
            None => push_diff(
                &mut lines,
                &mut texts,
                Some(diff),
                CommitLine::NoDiff(index),
                |hunk, line| CommitLine::Diff {
                    file: index,
                    hunk,
                    line,
                },
            ),
        }
    }
    let numbers = numbers(commit, &lines);
    (lines, numbers, texts.join("\n"))
}

/// `M src/lib.rs  +3 −1`.
fn file_header(path: &Path, diff: &FileDiff) -> String {
    let mut header = format!("{} {}", diff.state().letter(), path.display());
    if let Some(original) = diff.original() {
        let _ = write!(header, "  from {}", original.display());
    }
    if let Some(counts) = diff.line_counts() {
        let _ = write!(header, "  +{} −{}", counts.added, counts.removed);
    }
    header
}

fn numbers(commit: &Commit, lines: &[CommitLine]) -> Numbers {
    let (mut old, mut new) = (0, 0);
    lines
        .iter()
        .map(|line| {
            let CommitLine::Diff { file, hunk, line } = *line else {
                return (None, None);
            };
            let hunk = &commit.files[file].1.hunks[hunk];
            let Some(line) = line else {
                (old, new) = (hunk.old_start, hunk.new_start);
                return (None, None);
            };
            match hunk.lines[line].as_bytes().first() {
                Some(b'-') => {
                    old += 1;
                    (Some(old - 1), None)
                }
                Some(b'+') => {
                    new += 1;
                    (None, Some(new - 1))
                }
                Some(b'\\') => (None, None),
                _ => {
                    (old, new) = (old + 1, new + 1);
                    (Some(old - 1), Some(new - 1))
                }
            }
        })
        .collect()
}

/// Faces of `diff`'s hunk lines: the old side's lines and the new side's each highlighted
/// as code of their own, so neither breaks the other's syntax. Only the lines the buffer
/// shows, the first `MAX_DIFF_LINES`.
fn highlight_file(
    highlighters: &[CodeHighlighter],
    mode: &'static str,
    diff: &FileDiff,
) -> FileFaces {
    let shown = || {
        let hunks = diff.hunks.iter().enumerate();
        hunks
            .flat_map(|(index, hunk)| {
                let lines = hunk.lines.iter().enumerate();
                lines.map(move |(at, line)| (index, at, line))
            })
            .take(MAX_DIFF_LINES)
    };
    let mut faces = FileFaces::new();
    for (hunk, _, _) in shown() {
        faces.resize_with(faces.len().max(hunk + 1), Vec::new);
        faces[hunk].push(Vec::new());
    }
    for side in *b"-+" {
        let mut code = String::new();
        let mut placed = Vec::new();
        for (hunk, index, line) in shown() {
            let sign = line.as_bytes().first().copied();
            if sign != Some(b' ') && sign != Some(side) {
                continue;
            }
            let start = code.len();
            code.push_str(&line[1..]);
            // Lines both sides have take the new side's faces.
            if sign == Some(side) || side == b'+' {
                placed.push((hunk, index, start..code.len()));
            }
            code.push('\n');
        }
        if placed.is_empty() {
            continue;
        }
        let spans = highlighters
            .iter()
            .find_map(|highlight| highlight(mode, &code))
            .unwrap_or_default();
        let mut first = 0;
        for (hunk, line, bytes) in placed {
            while spans
                .get(first)
                .is_some_and(|span| span.range.end <= bytes.start)
            {
                first += 1;
            }
            faces[hunk][line] = spans[first..]
                .iter()
                .take_while(|span| span.range.start < bytes.end)
                .map(|span| Span {
                    range: span.range.start.max(bytes.start) - bytes.start
                        ..span.range.end.min(bytes.end) - bytes.start,
                    face: span.face,
                })
                .collect();
        }
    }
    faces
}

/// Faces of a commit buffer's `range`: file headers bold, hunk headers dim, each `-` and
/// `+` in its color and the code after it in its file's language.
pub fn decorate(buffer: &Buffer, range: Range<usize>) -> Vec<Span> {
    let Some(view) = buffer.local::<CommitView>() else {
        return Vec::new();
    };
    let text = buffer.text();
    let style = |style: ConsoleStyle| Face::Console(style);
    let mut spans = Vec::new();
    for line in text.line_of(range.start)..=text.line_of(range.end) {
        let (Some(&kind), Some(bytes)) = (view.lines.get(line), text.line_range(line)) else {
            continue;
        };
        match kind {
            CommitLine::File(_) => spans.push(Span {
                range: bytes,
                face: style(ConsoleStyle {
                    bold: true,
                    ..ConsoleStyle::default()
                }),
            }),
            CommitLine::Diff { line: None, .. } | CommitLine::NoDiff(_) => spans.push(Span {
                range: bytes,
                face: style(ConsoleStyle {
                    dim: true,
                    ..ConsoleStyle::default()
                }),
            }),
            CommitLine::Diff {
                file,
                hunk,
                line: Some(index),
            } => {
                let sign = match view.numbers[line] {
                    (Some(_), None) => Some(REMOVED_SIGN),
                    (None, Some(_)) => Some(ADDED_SIGN),
                    _ => None,
                };
                spans.extend(sign.map(|color| Span {
                    range: bytes.start..bytes.start + 1,
                    face: style(ConsoleStyle {
                        color: Some(ConsoleColor::Indexed(color)),
                        ..ConsoleStyle::default()
                    }),
                }));
                let code_start = bytes.start + 1;
                spans.extend(view.faces_of(file)[hunk][index].iter().map(|span| Span {
                    range: code_start + span.range.start..code_start + span.range.end,
                    face: span.face,
                }));
            }
        }
    }
    spans
        .into_iter()
        .filter_map(|span| {
            let shown = span.range.start.max(range.start)..span.range.end.min(range.end);
            (shown.start < shown.end).then_some(Span {
                range: shown,
                face: span.face,
            })
        })
        .collect()
}

/// Where each removed line and the added line taking its place differ: between what their
/// starts and ends share, like git's diff-highlight. Only for runs of as many lines added
/// as removed, so each line has a pair.
fn changed_parts(
    commit: &Commit,
    lines: &[CommitLine],
    numbers: &[(Option<usize>, Option<usize>)],
) -> HashMap<usize, Range<usize>> {
    let code = |line: usize| match lines[line] {
        CommitLine::Diff {
            file,
            hunk,
            line: Some(index),
        } => &commit.files[file].1.hunks[hunk].lines[index][1..],
        _ => "",
    };
    let run_end = |from: usize, side: (bool, bool)| {
        let is_side = |at: &usize| (numbers[*at].0.is_some(), numbers[*at].1.is_some()) == side;
        (from..lines.len())
            .find(|at| !is_side(at))
            .unwrap_or(lines.len())
    };
    let mut changed = HashMap::new();
    let mut line = 0;
    while line < lines.len() {
        let removed = line..run_end(line, (true, false));
        let added = removed.end..run_end(removed.end, (false, true));
        if !removed.is_empty() && removed.len() == added.len() {
            for (old, new) in removed.zip(added.clone()) {
                let Some(parts) = changed_between(code(old), code(new)) else {
                    continue;
                };
                for (at, part) in [(old, parts.0), (new, parts.1)] {
                    if !part.is_empty() {
                        // Past the `-` or `+`.
                        changed.insert(at, part.start + 1..part.end + 1);
                    }
                }
            }
        }
        line = added.end.max(line + 1);
    }
    changed
}

/// What lies between the start and the end `old` and `new` share, in each; none when they
/// share neither, as the whole line changed. Shared indentation or punctuation alone,
/// like `    ` and `;`, does not count.
fn changed_between(old: &str, new: &str) -> Option<(Range<usize>, Range<usize>)> {
    let shared = |old: &mut dyn Iterator<Item = char>, new: &mut dyn Iterator<Item = char>| {
        old.zip(new)
            .take_while(|(old, new)| old == new)
            .map(|(old, _)| old.len_utf8())
            .sum::<usize>()
    };
    let start = shared(&mut old.chars(), &mut new.chars());
    let end = shared(
        &mut old[start..].chars().rev(),
        &mut new[start..].chars().rev(),
    );
    let real = |part: &str| part.chars().any(char::is_alphanumeric);
    (real(&old[..start]) || real(&old[old.len() - end..]))
        .then(|| (start..old.len() - end, start..new.len() - end))
}

/// The changed part of each paired line, a layer over the faces of [`decorate`].
pub fn decorate_changes(buffer: &Buffer, range: Range<usize>) -> Vec<Span> {
    let Some(view) = buffer.local::<CommitView>() else {
        return Vec::new();
    };
    let text = buffer.text();
    (text.line_of(range.start)..=text.line_of(range.end))
        .filter_map(|line| {
            let part = view.changed.get(&line)?;
            let start = text.line_start(line)?;
            let shown = (start + part.start).max(range.start)..(start + part.end).min(range.end);
            let face = match view.numbers[line] {
                (Some(_), None) => Face::Removed,
                _ => Face::Added,
            };
            (shown.start < shown.end).then_some(Span { range: shown, face })
        })
        .collect()
}

/// Each line's old and new numbers in the gutter, and the bands of headers and of added
/// and removed lines.
pub fn line_extras(editor: &Editor, lines: Range<usize>) -> Vec<LineExtra> {
    let Some(view) = editor.window_buffer().local::<CommitView>() else {
        return Vec::new();
    };
    let number = |number: Option<usize>| number.map_or_else(String::new, |at| at.to_string());
    lines
        .filter_map(|line| {
            let kind = *view.lines.get(line)?;
            let (old, new) = view.numbers[line];
            let fill = match (kind, old, new) {
                (CommitLine::File(_), ..) => Some(Tone::Faint),
                (CommitLine::Diff { .. }, Some(_), None) => Some(Tone::Danger),
                (CommitLine::Diff { .. }, None, Some(_)) => Some(Tone::Success),
                _ => None,
            };
            let width = view.digits;
            Some(LineExtra {
                line,
                fill,
                number: Some(format!("{:>width$} {:>width$}", number(old), number(new))),
                ..LineExtra::default()
            })
        })
        .collect()
}

/// The commit's facts, message and files beside its diff, and its keys.
pub fn controls(editor: &Editor) -> Option<TextControls> {
    let buffer = editor.window_buffer();
    let view = buffer.local::<CommitView>()?;
    Some(TextControls {
        hints: HINTS,
        side: Some(side(view, buffer.point_line())),
        number_width: Some(2 * view.digits + 1),
        ..TextControls::default()
    })
}

/// The file at `point_line` is the one selected.
fn side(view: &CommitView, point_line: usize) -> TextSide {
    let (rows, actions) = view.side_rows.clone();
    let first_file = rows.len() - view.commit.files.len();
    let selected = view
        .lines
        .get(point_line)
        .map(|line| first_file + line.file());
    TextSide {
        facts: facts(view),
        rows,
        actions,
        selected,
    }
}

type SideRows = (Rc<[OutlineRow]>, Rc<[Option<Action>]>);

/// The message, unless `folded`, and the files beside the diff, with what a click on each
/// does; kept, so a frame does not build a row per file.
fn side_rows(commit: &Commit, folded: bool) -> SideRows {
    let mut rows: Vec<(OutlineRow, Option<Action>)> = Vec::new();
    if !commit.body.is_empty() {
        let heading = OutlineRow::Heading {
            title: "Message".into(),
            count: commit.body.len(),
            open: !folded,
            tone: Tone::Plain,
        };
        rows.push((heading, Some(Rc::new(toggle))));
        if !folded {
            rows.extend(
                (commit.body.iter())
                    .map(|text| (OutlineRow::Diff(DiffRow::Context(text.clone())), None)),
            );
        }
    }
    let heading = OutlineRow::Heading {
        title: "Files".into(),
        count: commit.files.len(),
        open: true,
        tone: Tone::Plain,
    };
    rows.push((heading, None));
    for (index, (path, diff)) in commit.files.iter().enumerate() {
        let row = OutlineRow::File {
            state: diff.state(),
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            directory: directory_label(path),
            lines: diff.line_counts(),
            note: diff
                .original()
                .filter(|_| diff.hunks.is_empty())
                .map(|original| (format!("from {}", original.display()), Tone::Faint)),
            open: false,
        };
        let action: Action = Rc::new(move |editor| go_to_file(editor, index));
        rows.push((row, Some(action)));
    }
    let (rows, actions): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
    (rows.into(), actions.into())
}

fn facts(view: &CommitView) -> Vec<Fact> {
    let commit = &view.commit;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    let mut facts = vec![
        Fact {
            label: "Commit",
            value: FactValue::Code(short(&commit.id)),
            note: commit.subject.clone(),
            note_is_title: true,
            buttons: Vec::new(),
        },
        Fact {
            label: "Author",
            value: FactValue::Code(commit.author.clone()),
            note: format!("{} · {}", commit.date, ago(now - commit.time)),
            note_is_title: false,
            buttons: Vec::new(),
        },
    ];
    for (index, (id, subject)) in commit.parents.iter().enumerate() {
        let (root, id_to_open) = (view.root.clone(), id.clone());
        facts.push(Fact {
            label: if index == 0 { "Parent" } else { "Merged" },
            value: FactValue::Code(short(id)),
            note: subject.clone(),
            note_is_title: false,
            buttons: vec![Button {
                label: "Open".into(),
                selected: false,
                action: Rc::new(move |editor| open(editor, &root, &id_to_open, None)),
            }],
        });
    }
    facts
}

fn short(id: &str) -> String {
    id.chars().take(SHORT_ID).collect()
}

/// Folds or unfolds the commit's message beside its diff.
pub fn toggle(editor: &mut Editor) -> CommandResult {
    let view = editor
        .window_buffer_mut()
        .local_mut::<CommitView>()
        .ok_or(NOT_A_COMMIT_BUFFER)?;
    if view.commit.body.is_empty() {
        return Err("The message is only its subject, shown beside Commit".into());
    }
    view.message_folded = !view.message_folded;
    view.side_rows = side_rows(&view.commit, view.message_folded);
    Ok(())
}

/// Puts point on the header of the commit's `file`, at the top of the window.
fn go_to_file(editor: &mut Editor, file: usize) -> CommandResult {
    let view = editor
        .window_buffer()
        .local::<CommitView>()
        .ok_or(NOT_A_COMMIT_BUFFER)?;
    let line = (view.lines.iter())
        .position(|line| *line == CommitLine::File(file))
        .ok_or("No such file")?;
    editor.set_point_at(line, 0);
    editor.scroll(line as isize - editor.viewport.first_line as isize);
    Ok(())
}

pub fn next_hunk(editor: &mut Editor) -> CommandResult {
    move_to(editor, true, is_hunk, "No next hunk")
}

pub fn previous_hunk(editor: &mut Editor) -> CommandResult {
    move_to(editor, false, is_hunk, "No previous hunk")
}

pub fn next_file(editor: &mut Editor) -> CommandResult {
    move_to(editor, true, is_file, "No next file")
}

pub fn previous_file(editor: &mut Editor) -> CommandResult {
    move_to(editor, false, is_file, "No previous file")
}

fn is_hunk(line: &CommitLine) -> bool {
    matches!(line, CommitLine::Diff { line: None, .. })
}

fn is_file(line: &CommitLine) -> bool {
    matches!(line, CommitLine::File(_))
}

/// Puts point on the first line after it that `wanted` holds for, or with `forward` false
/// the last before it.
fn move_to(
    editor: &mut Editor,
    forward: bool,
    wanted: fn(&CommitLine) -> bool,
    none: &'static str,
) -> CommandResult {
    move_to_line(editor, commit_lines, forward, wanted, none)
}

fn commit_lines(buffer: &Buffer) -> Result<&[CommitLine], &'static str> {
    let view = buffer.local::<CommitView>().ok_or(NOT_A_COMMIT_BUFFER)?;
    Ok(&view.lines)
}

/// Like [`move_to`], of the window buffer's `lines_of`.
pub fn move_to_line<L>(
    editor: &mut Editor,
    lines_of: fn(&Buffer) -> Result<&[L], &'static str>,
    forward: bool,
    wanted: fn(&L) -> bool,
    none: &'static str,
) -> CommandResult {
    let buffer = editor.window_buffer();
    let point_line = buffer.point_line();
    let lines = lines_of(buffer)?;
    let target = if forward {
        (lines.iter().skip(point_line + 1))
            .position(wanted)
            .map(|at| point_line + 1 + at)
    } else {
        lines[..point_line.min(lines.len())]
            .iter()
            .rposition(wanted)
    };
    go_to(editor, target.ok_or(none)?);
    Ok(())
}

/// Puts point on `line`; one out of sight is scrolled to the top, so what follows it shows.
fn go_to(editor: &mut Editor, line: usize) {
    let viewport = editor.viewport;
    let in_sight = (viewport.first_line..viewport.first_line + viewport.height).contains(&line);
    editor.set_point_at(line, 0);
    if !in_sight {
        editor.scroll(line as isize - editor.viewport.first_line as isize);
    }
}

/// The commit's file at point, and on a diff line, the line of the file it is at.
fn file_at_point(editor: &Editor) -> Result<(&CommitView, usize, Option<usize>), &'static str> {
    let buffer = editor.window_buffer();
    let view = buffer.local::<CommitView>().ok_or(NOT_A_COMMIT_BUFFER)?;
    let line = *view.lines.get(buffer.point_line()).ok_or("Nothing here")?;
    let target = match line {
        CommitLine::Diff { file, hunk, line } => {
            (view.commit.files[file].1.hunks.get(hunk)).map(|hunk| hunk.new_line(line.unwrap_or(0)))
        }
        _ => None,
    };
    Ok((view, line.file(), target))
}

/// Visits the file at point in the working tree, on a diff line at that line, like
/// Emacs `magit-diff-visit-worktree-file`.
pub fn visit(editor: &mut Editor) -> CommandResult {
    let (view, file, target) = file_at_point(editor)?;
    let path = view.root.join(&view.commit.files[file].0);
    if !path.exists() {
        return Err("Not in the working tree any more; o shows it as committed".into());
    }
    editor.visit_file(&path)?;
    go_to_line(editor, target);
    Ok(())
}

/// Shows the file at point as the commit left it, or as it was before a commit that
/// deleted it, like Emacs `magit-diff-visit-file`.
pub fn open_file(editor: &mut Editor) -> CommandResult {
    let (view, file, target) = file_at_point(editor)?;
    let (path, diff) = &view.commit.files[file];
    if diff.binary {
        return Err("Binary file".into());
    }
    let id = short(&view.commit.id);
    let revision = if diff.state() == FileState::Deleted {
        format!("{}^", view.commit.id)
    } else {
        view.commit.id.clone()
    };
    let root = view.root.clone();
    let name = format!(
        "{} @ {id}",
        path.file_name().unwrap_or(path.as_os_str()).display()
    );
    let path = path.clone();
    if editor.switch_to_buffer(&name) {
        go_to_line(editor, target);
        return Ok(());
    }
    let mut spec = revision.into_bytes();
    spec.push(b':');
    spec.extend_from_slice(path.as_os_str().as_encoded_bytes());
    let read_root = root.clone();
    reads::spawn(
        editor,
        format!("Reading {name}…"),
        move || {
            output(
                git(&read_root)
                    .arg("show")
                    .arg(OsStr::from_bytes(&spec))
                    .arg("--"),
            )
            .map_err(|error| format!("git show: {error}"))
        },
        move |editor, contents| {
            // Opened again while it was read.
            if !editor.switch_to_buffer(&name) {
                let mode = editor.mode_for(Some(&root.join(&path)));
                let directory = root.join(&path).parent().unwrap_or(&root).to_path_buf();
                let buffer = Buffer::new(
                    name,
                    Text::from(String::from_utf8_lossy(&contents).as_ref()),
                    mode,
                )
                .with_directory(directory)
                .read_only();
                editor.add_buffer(buffer);
            }
            go_to_line(editor, target);
            Ok(())
        },
    );
    Ok(())
}

/// Puts point at the start of `line`, from 1.
fn go_to_line(editor: &mut Editor, line: Option<usize>) {
    if let Some(line) = line {
        editor.set_point_at(line.saturating_sub(1), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::parse_diff;

    /// `let` at the start of the code, and a span from byte 8 across the first newline.
    fn fake_highlighter() -> CodeHighlighter {
        Rc::new(|_, code: &str| {
            let span = |range, face| Span { range, face };
            Some(vec![
                span(0..3, Face::Keyword),
                span(8..14.min(code.len()), Face::Comment),
            ])
        })
    }

    #[test]
    fn each_line_takes_its_sides_faces_cut_at_the_line() {
        let diff = parse_diff("@@ -1,2 +1,2 @@\n let a = 1;\n-let b = 2;\n+let b = 3;\n").unwrap();
        let faces = highlight_file(&[fake_highlighter()], "Rust", &diff);
        let face = |range, face| Span { range, face };
        assert_eq!(
            faces[0],
            [
                vec![face(0..3, Face::Keyword), face(8..10, Face::Comment)],
                // Each side's second line starts at byte 11 of its code.
                vec![face(0..3, Face::Comment)],
                vec![face(0..3, Face::Comment)],
            ]
        );
    }

    #[test]
    fn paired_lines_mark_what_changed_between_what_they_share() {
        let diff = parse_diff(
            "@@ -1,6 +1,5 @@\n let a = 1;\n-let b = 2;\n+let b = 30;\n-x\n-y\n+z\n-gone\n+new\n",
        )
        .unwrap();
        let commit = Commit {
            id: String::new(),
            parents: Vec::new(),
            author: String::new(),
            time: 0,
            date: String::new(),
            subject: String::new(),
            body: Vec::new(),
            files: vec![(PathBuf::from("a.rs"), diff)],
        };
        let (lines, numbers, _) = layout(&commit);
        let changed = changed_parts(&commit, &lines, &numbers);
        // Lines: the file, the hunk, ` let a`, `-let b`, `+let b`, `-x`, `-y`, `+z`, ...
        // Two removed for one added are not paired; `gone` and `new` share nothing.
        let mut found: Vec<_> = changed.into_iter().collect();
        found.sort_by_key(|(line, _)| *line);
        assert_eq!(found, [(3, 9..10), (4, 9..11)]);
        assert_eq!(changed_between("aé1b", "aé22b"), Some((3..4, 3..5)));
        assert_eq!(changed_between("ab", "abc"), Some((2..2, 2..3)));
        assert_eq!(changed_between("    let x = 1;", "    return y;"), None);
    }

    #[test]
    fn only_the_lines_shown_are_highlighted() {
        let added: String = (0..MAX_DIFF_LINES + 5).map(|_| "+x\n").collect();
        let diff = parse_diff(&format!("@@ -0,0 +1,9 @@\n{added}")).unwrap();
        let faces = highlight_file(&[fake_highlighter()], "Rust", &diff);
        assert_eq!(faces.iter().map(Vec::len).sum::<usize>(), MAX_DIFF_LINES);
    }
}
