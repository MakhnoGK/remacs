//! The files open in each project, with point and scroll, kept when quitting and opened
//! again on the next start.

use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use remacs_core::{Editor, quote};
use serde::{Deserialize, Serialize};

use crate::ignored::FilesSettings;
use crate::project::{abbreviate_home, project_root};

#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct Projects {
    /// The project shown when last quitting, opened again when no path is given.
    last: Option<PathBuf>,
    /// Each project's files, most recently selected first.
    files: BTreeMap<PathBuf, Vec<OpenFile>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct OpenFile {
    path: PathBuf,
    line: usize,
    column: usize,
    first_line: usize,
    /// The lines its folds hid, hidden again while the file has as many lines as then.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    folded: Vec<Range<usize>>,
    #[serde(default)]
    line_count: usize,
}

#[derive(Default)]
struct Session {
    projects: Projects,
    store: Option<PathBuf>,
}

/// Keeps the session in `store` from now on, first opening the files kept there for the
/// project of `opening`, else of the last one, unless `restore-session` is off.
pub fn keep_session_in(editor: &mut Editor, store: PathBuf, opening: Option<&Path>) {
    let projects: Projects = fs::read_to_string(&store)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let root = match opening {
        Some(path) => std::path::absolute(path).ok().map(|path| project_of(&path)),
        None => projects.last.clone(),
    };
    if editor.settings_section::<FilesSettings>().restore_session
        && let Some(files) = root.and_then(|root| projects.files.get(&root))
    {
        reopen(editor, files);
    }
    *editor.plugin_state() = Session { projects, store: Some(store) };
}

fn reopen(editor: &mut Editor, files: &[OpenFile]) {
    let mut skipped = Vec::new();
    // Least recent first, so that the most recent ends up shown.
    for file in files.iter().rev() {
        if !file.path.is_file() || editor.visit_file(&file.path).is_err() {
            skipped.push(quote(&abbreviate_home(&file.path)));
            continue;
        }
        let line_count = editor.window_buffer().text().line_count();
        // A file changed since may have other lines where these were.
        if line_count == file.line_count {
            editor.window_buffer_mut().fold(file.folded.iter().cloned());
        }
        editor.set_point_at(file.line, file.column);
        editor.viewport.first_line = file.first_line.min(line_count.saturating_sub(1));
    }
    if !skipped.is_empty() {
        editor.set_message(format!("Not reopened: {}", skipped.join(", ")));
    }
}

/// Keeps the files open, then quits.
pub fn quit(editor: &mut Editor) {
    save(editor);
    editor.request_quit();
}

fn save(editor: &mut Editor) {
    let mut open = BTreeMap::<PathBuf, Vec<OpenFile>>::new();
    for (index, buffer) in editor.buffers().iter().enumerate() {
        let Some(path) = buffer.path().filter(|path| path.is_file()) else {
            continue;
        };
        let first_line = if index == 0 { editor.viewport.first_line } else { buffer.window_start };
        open.entry(project_of(path)).or_default().push(OpenFile {
            path: path.to_owned(),
            line: buffer.point_line(),
            column: buffer.point_column(),
            first_line,
            folded: buffer.folded_lines(),
            line_count: buffer.text().line_count(),
        });
    }
    let shown = editor.window_buffer().default_directory().map(project_root);
    let session = editor.plugin_state::<Session>();
    let Some(store) = session.store.clone() else {
        return;
    };
    let projects = &mut session.projects;
    // shortcut: only the shown project forgets files closed since; track every project
    // visited this session if stale files reopening in others annoys.
    if let Some(shown) = shown {
        // Its files all closed, it opens none next time.
        projects.files.remove(&shown);
        projects.last = Some(shown);
    }
    projects.files.extend(open);
    let written =
        serde_json::to_string_pretty(projects).map_err(std::io::Error::other).and_then(|json| {
            store.parent().map_or(Ok(()), fs::create_dir_all)?;
            fs::write(&store, json)
        });
    if let Err(error) = written {
        eprintln!("remacs: {}: {error}", store.display());
    }
}

fn project_of(path: &Path) -> PathBuf {
    if path.is_dir() { project_root(path) } else { project_root(path.parent().unwrap_or(path)) }
}
