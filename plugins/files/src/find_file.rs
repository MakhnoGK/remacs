use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_core::{
    Buffer, Candidate, CommandResult, Completions, Count, Editor, Icon, Prompt, filter, rank,
};

use crate::project::{
    abbreviate_home, default_directory, expand_home, list_files, project_root, relative_directory,
    with_slash,
};

const FILE_CATEGORY: &str = "file";
/// Matches turned into rows; the counter still reports all of them.
const MAX_CANDIDATES: usize = 500;

/// Opens a file of the current project, matched by name (or by path once the input has
/// a `/`); input starting with `/` or `~` browses the file system instead.
pub fn find_file(editor: &mut Editor) -> CommandResult {
    let root = project_root(&default_directory(editor));
    let files = Rc::new(list_files(&root));
    read_file_name(editor, Rc::new(root), files, String::new())
}

fn read_file_name(
    editor: &mut Editor,
    root: Rc<PathBuf>,
    files: Rc<Vec<String>>,
    initial_input: String,
) -> CommandResult {
    let prompt = Prompt {
        label: "Find file".into(),
        category: FILE_CATEGORY,
        prefix: with_slash(&abbreviate_home(&root)),
        initial_input,
        count: Count::Matches,
        wide: true,
        hints: &[("↵", "Open"), ("⇥", "Complete path"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    let (source_root, source_files) = (Rc::clone(&root), Rc::clone(&files));
    editor.read(
        prompt,
        move |editor, input| {
            if is_absolute(input) {
                directory_candidates(input)
            } else {
                project_candidates(editor, &source_root, &source_files, input)
            }
        },
        move |editor, answer| {
            let path = if is_absolute(&answer) {
                expand_home(&answer)
            } else {
                root.join(&answer)
            };
            if path.is_dir() {
                let input = with_slash(&abbreviate_home(&path));
                return read_file_name(editor, root, files, input);
            }
            Ok(editor.visit_file(&path)?)
        },
    )
}

fn project_candidates(editor: &Editor, root: &Path, files: &[String], input: &str) -> Completions {
    let visited: HashSet<&Path> = editor.buffers().iter().filter_map(Buffer::path).collect();
    let by_path = input.contains('/');
    let mut matches = filter(input, files, |file| {
        if by_path {
            file.as_str()
        } else {
            file_name(file)
        }
    });
    rank(&mut matches);
    let total = matches.len();
    let candidates = matches
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|matched| {
            let path = root.join(matched.item);
            let (label, annotation) = if by_path {
                (matched.item.clone(), String::new())
            } else {
                let directory = path.parent().unwrap_or(root);
                (
                    file_name(matched.item).to_owned(),
                    relative_directory(root, directory),
                )
            };
            Candidate {
                label,
                matches: matched.ranges,
                annotation,
                icon: Some(Icon::File),
                tag: visited.contains(path.as_path()).then_some("open"),
                ..Candidate::new(matched.item.as_str())
            }
        })
        .collect();
    Completions { candidates, total }
}

/// Entries of the directory typed so far, matching the name being typed.
fn directory_candidates(input: &str) -> Completions {
    let split = input.rfind('/').map_or(0, |slash| slash + 1);
    let (typed_directory, partial_name) = input.split_at(split);
    let Ok(entries) = fs::read_dir(expand_home(typed_directory)) else {
        return Completions::default();
    };
    let show_hidden = partial_name.starts_with('.');
    let mut entries: Vec<(String, bool)> = entries
        .flatten()
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, entry.path().is_dir())
        })
        .filter(|(name, _)| show_hidden || !name.starts_with('.'))
        .collect();
    entries.sort();
    let mut matches = filter(partial_name, entries, |(name, _)| name);
    rank(&mut matches);
    matches
        .into_iter()
        .map(|matched| {
            let (name, is_directory) = matched.item;
            let label = if is_directory {
                format!("{name}/")
            } else {
                name
            };
            Candidate {
                matches: matched.ranges,
                icon: Some(if is_directory {
                    Icon::Directory
                } else {
                    Icon::File
                }),
                label: label.clone(),
                ..Candidate::new(format!("{typed_directory}{label}"))
            }
        })
        .collect::<Vec<_>>()
        .into()
}

fn is_absolute(input: &str) -> bool {
    input.starts_with('/') || input.starts_with('~')
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
