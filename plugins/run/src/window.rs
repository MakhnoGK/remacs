//! What the run plugin knows of the window buffer: its project, and its entry points,
//! found again once edits pause.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use remacs_core::{Editor, GutterIcon, GutterMark};
use remacs_files::project_root;
use remacs_text::Snapshot;

use crate::console::Console;
use crate::entry_points::{EntryPoint, may_have_entry_points};
use crate::scanner::{Request, ScanSlot, Scanner};
use crate::sessions::{get, sessions};

pub const ENTRY_POINT_MENU: &str = "run-entry-point-menu";

/// A buffer's entry points, as of a version of its text.
pub struct EntryPoints {
    text: Snapshot,
    pub root: PathBuf,
    pub points: Vec<EntryPoint>,
}

/// The version of a buffer's text last sent to the scanner, and where its entry points
/// will be left.
struct Scanning {
    slot: ScanSlot,
    text: Snapshot,
    root: PathBuf,
}

#[derive(Default)]
pub struct Window {
    /// The window buffer's name and project root, if it has one.
    root: Option<(String, Option<PathBuf>)>,
    /// The window buffer's text changed: its name and the command count then.
    stale: Option<(String, u64)>,
}

/// The project of the window buffer: a console's run's, else the buffer's directory's.
pub fn window_root(editor: &Editor) -> Option<PathBuf> {
    let window = editor.find_plugin_state::<Window>()?;
    let (name, root) = window.root.as_ref()?;
    (name == editor.window_buffer().name()).then(|| root.clone()).flatten()
}

pub fn update(editor: &mut Editor) {
    let name = editor.window_buffer().name().to_owned();
    let known = editor
        .find_plugin_state::<Window>()
        .and_then(|window| window.root.as_ref())
        .is_some_and(|(known, _)| *known == name);
    if !known {
        let root = find_root(editor);
        editor.plugin_state::<Window>().root = Some((name, root));
    }
    update_entry_points(editor);
}

fn find_root(editor: &Editor) -> Option<PathBuf> {
    let buffer = editor.window_buffer();
    if let Some(console) = buffer.local::<Console>() {
        return get(editor, console.session).map(|session| session.root.clone());
    }
    buffer.default_directory().map(project_root)
}

/// Finds the window buffer's entry points when it is first shown, and again once its
/// text has stayed the same for the delay `run.toml` sets.
fn update_entry_points(editor: &mut Editor) {
    take_scan(editor);
    let buffer = editor.window_buffer();
    let (Some(path), Some(root)) = (buffer.path().map(Path::to_path_buf), window_root(editor))
    else {
        return;
    };
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    let mode = buffer.mode();
    if !may_have_entry_points(mode, file_name) {
        return;
    }
    let known = buffer
        .local::<EntryPoints>()
        .is_some_and(|known| known.text.is_same(buffer.text()) && known.root == root);
    let asked = buffer
        .local::<Scanning>()
        .is_some_and(|scanning| scanning.text.is_same(buffer.text()) && scanning.root == root);
    if known || asked {
        return;
    }
    let name = buffer.name().to_owned();
    let count = editor.command_count();
    let quiet = buffer.local::<EntryPoints>().is_none()
        || editor
            .find_plugin_state::<Window>()
            .and_then(|window| window.stale.as_ref())
            .is_some_and(|stale| *stale == (name.clone(), count));
    if !quiet {
        editor.plugin_state::<Window>().stale = Some((name, count));
        let delay = sessions(editor).settings.entry_point_delay();
        editor.refresh_after(delay);
        return;
    }
    editor.plugin_state::<Window>().stale = None;
    let buffer = editor.window_buffer_mut();
    let slot =
        buffer.local::<Scanning>().map(|scanning| Arc::clone(&scanning.slot)).unwrap_or_default();
    let text = buffer.text().snapshot();
    buffer.set_local(Scanning { slot: Arc::clone(&slot), text: text.clone(), root: root.clone() });
    let request = Request { path, mode, text, root, slot };
    let notifier = editor.notifier();
    editor.plugin_state::<Scanner>().send(request, notifier);
}

/// Takes the window buffer's entry points the scanner found since the last call.
fn take_scan(editor: &mut Editor) {
    let buffer = editor.window_buffer_mut();
    let Some(scan) = buffer
        .local::<Scanning>()
        .and_then(|scanning| scanning.slot.lock().expect("scan slots are never poisoned").take())
    else {
        return;
    };
    buffer.set_local(EntryPoints { text: scan.text, root: scan.root, points: scan.points });
}

/// The window buffer's entry points, as last found.
pub fn window_entry_points(editor: &Editor) -> Option<&EntryPoints> {
    editor.window_buffer().local::<EntryPoints>()
}

/// The entry point on `line`, else the innermost around it, like the test point is in,
/// else the buffer's first, as the one meant there.
pub fn entry_point_for(editor: &Editor, line: usize) -> Option<(PathBuf, EntryPoint)> {
    let known = window_entry_points(editor)?;
    let point = known
        .points
        .iter()
        .find(|point| point.line == line)
        .or_else(|| {
            known
                .points
                .iter()
                .filter(|point| (point.line..=point.end_line).contains(&line))
                .min_by_key(|point| point.end_line - point.line)
        })
        .or_else(|| known.points.first())?;
    Some((known.root.clone(), point.clone()))
}

/// ▶ beside the window buffer's entry points; nothing for buffers that cannot have any.
pub fn gutter_marks(editor: &Editor, lines: std::ops::Range<usize>) -> Option<Vec<GutterMark>> {
    let known = window_entry_points(editor)?;
    Some(
        known
            .points
            .iter()
            .filter(|point| lines.contains(&point.line))
            .map(|point| GutterMark {
                line: point.line,
                icon: GutterIcon::Run,
                command: ENTRY_POINT_MENU,
            })
            .collect(),
    )
}
