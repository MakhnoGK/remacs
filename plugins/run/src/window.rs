//! What the run plugin knows of the window buffer: its project, and its entry points,
//! found again once edits pause.

use std::path::{Path, PathBuf};

use remacs_core::{Editor, GutterIcon, GutterMark};
use remacs_files::project_root;
use remacs_text::Snapshot;

use crate::console::Console;
use crate::entry_points::{EntryPoint, entry_points, may_have_entry_points};
use crate::sessions::{get, sessions};

/// Larger files are not looked through for entry points.
const MAX_SCANNED_BYTES: usize = 4 << 20;

pub const ENTRY_POINT_MENU: &str = "run-entry-point-menu";

/// A buffer's entry points, as of a version of its text.
pub struct EntryPoints {
    text: Snapshot,
    pub root: PathBuf,
    pub points: Vec<EntryPoint>,
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
    (name == editor.window_buffer().name())
        .then(|| root.clone())
        .flatten()
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
    let buffer = editor.window_buffer();
    let (Some(path), Some(root)) = (buffer.path().map(Path::to_path_buf), window_root(editor))
    else {
        return;
    };
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let mode = buffer.mode();
    if !may_have_entry_points(mode, file_name) {
        return;
    }
    let current = buffer
        .local::<EntryPoints>()
        .is_some_and(|known| known.text.is_same(buffer.text()) && known.root == root);
    if current {
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
    let buffer = editor.window_buffer();
    let text = buffer.text().snapshot();
    let points = if text.len() > MAX_SCANNED_BYTES {
        Vec::new()
    } else {
        let contents = buffer.contents(0..text.len());
        entry_points(&path, mode, &contents, &root)
    };
    editor
        .window_buffer_mut()
        .set_local(EntryPoints { text, root, points });
}

/// The window buffer's entry points, as last found.
pub fn window_entry_points(editor: &Editor) -> Option<&EntryPoints> {
    editor.window_buffer().local::<EntryPoints>()
}

/// The entry point on `line`, else the buffer's first, as the one meant there.
pub fn entry_point_for(editor: &Editor, line: usize) -> Option<(PathBuf, EntryPoint)> {
    let known = window_entry_points(editor)?;
    let point = known
        .points
        .iter()
        .find(|point| point.line == line)
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
