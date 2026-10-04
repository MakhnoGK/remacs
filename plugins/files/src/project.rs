use std::env;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ignore::{WalkBuilder, WalkState};
use remacs_core::Editor;

/// Keeps a prompt opened over a huge tree responsive.
const MAX_FILES: usize = 100_000;

/// Directory of the window's file, else the working directory.
pub fn default_directory(editor: &Editor) -> PathBuf {
    editor
        .window_buffer()
        .path()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// The nearest enclosing Git work tree, else `directory` itself.
pub fn project_root(directory: &Path) -> PathBuf {
    let directory = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.to_path_buf());
    directory
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .unwrap_or(&directory)
        .to_path_buf()
}

/// Files under `root`, relative to it, without hidden and Git-ignored ones;
/// shallow files first, then by path.
pub fn list_files(root: &Path) -> Vec<String> {
    let files = Mutex::new(Vec::new());
    WalkBuilder::new(root).build_parallel().run(|| {
        Box::new(|entry| {
            let Ok(entry) = entry else {
                return WalkState::Continue;
            };
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                return WalkState::Continue;
            }
            let mut files = files.lock().expect("walker threads do not panic");
            if files.len() >= MAX_FILES {
                return WalkState::Quit;
            }
            if let Ok(relative) = entry.path().strip_prefix(root) {
                files.push(relative.to_string_lossy().into_owned());
            }
            WalkState::Continue
        })
    });
    let mut files = files.into_inner().expect("walker threads do not panic");
    let depth = |path: &String| path.matches('/').count();
    files.sort_by(|a, b| depth(a).cmp(&depth(b)).then_with(|| a.cmp(b)));
    files
}

/// `dir/` relative to `root`, `./` for the root itself, else from `~`.
pub fn relative_directory(root: &Path, directory: &Path) -> String {
    match directory.strip_prefix(root) {
        Ok(relative) if relative.as_os_str().is_empty() => "./".into(),
        Ok(relative) => format!("{}/", relative.display()),
        Err(_) => with_slash(&abbreviate_home(directory)),
    }
}

pub fn abbreviate_home(path: &Path) -> String {
    match home().and_then(|home| path.strip_prefix(home).ok()) {
        Some(relative) if relative.as_os_str().is_empty() => "~".into(),
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

pub fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix('~'), home()) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            PathBuf::from(format!("{}{rest}", home.display()))
        }
        _ => PathBuf::from(path),
    }
}

pub fn with_slash(directory: &str) -> String {
    if directory.ends_with('/') {
        directory.to_owned()
    } else {
        format!("{directory}/")
    }
}

fn home() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}
