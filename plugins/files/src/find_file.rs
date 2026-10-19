use std::cmp::Reverse;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use remacs_core::{
    Buffer, Candidate, CommandResult, CompletionMemo, Completions, Count, Editor, FuzzyQuery, Icon,
    Prompt, filter, filter_map_parallel, rank,
};

use crate::file_index::{FileIndex, FileIndexes};
use crate::project::{abbreviate_home, default_directory, expand_home, project_root, with_slash};

pub const FILE_CATEGORY: &str = "file";
/// Matches turned into rows; the counter still reports all of them.
const MAX_CANDIDATES: usize = 500;

/// Best first: the higher score, then the file name matching too, as fzf's
/// `--scheme=path` ranks, then the shorter path, then the index's order.
type FileRank = (Reverse<u32>, bool, usize, usize);

/// Opens a file of the current project, matched fzf-style over its whole relative path;
/// input starting with `/` or `~` browses the file system instead.
pub fn find_file(editor: &mut Editor) -> CommandResult {
    let root = project_root(&default_directory(editor));
    let index = project_files(editor, &root);
    read_file_name(editor, Rc::new(root), index, String::new())
}

/// The files of the project at `root`, indexed in the background from the first call on
/// and kept current.
pub fn project_files(editor: &mut Editor, root: &Path) -> Arc<FileIndex> {
    let notifier = editor.notifier();
    let cache = editor.project_cache_dir(root);
    editor
        .plugin_state::<FileIndexes>()
        .get_or_start(root, cache, notifier)
}

fn read_file_name(
    editor: &mut Editor,
    root: Rc<PathBuf>,
    index: Arc<FileIndex>,
    initial_input: String,
) -> CommandResult {
    let prompt = Prompt {
        label: "Find file".into(),
        category: FILE_CATEGORY,
        directory: with_slash(&abbreviate_home(&root)),
        initial_input,
        count: Count::Matches,
        wide: true,
        hints: &[("↵", "Open"), ("⇥", "Complete path"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    let (source_root, source_index) = (Rc::clone(&root), Arc::clone(&index));
    let memo = CompletionMemo::<(String, Arc<Vec<String>>, usize)>::default();
    editor.read(
        prompt,
        move |editor, input| {
            if is_absolute(input) {
                return directory_candidates(input);
            }
            let files = source_index.files();
            // Open buffers tag their files.
            let key = (input.to_owned(), Arc::clone(&files), editor.buffers().len());
            memo.get(
                key,
                |last, new| last.0 == new.0 && Arc::ptr_eq(&last.1, &new.1) && last.2 == new.2,
                || project_candidates(editor, &source_root, &files, input),
            )
        },
        move |editor, answer| {
            let path = if is_absolute(&answer) {
                expand_home(&answer)
            } else {
                root.join(&answer)
            };
            if path.is_dir() {
                let input = with_slash(&abbreviate_home(&path));
                return read_file_name(editor, root, index, input);
            }
            Ok(editor.visit_file(&path)?)
        },
    )
}

fn project_candidates(editor: &Editor, root: &Path, files: &[String], input: &str) -> Completions {
    let visited: HashSet<&Path> = editor.buffers().iter().filter_map(Buffer::path).collect();
    let query = FuzzyQuery::for_paths(input);
    let mut matches = filter_map_parallel(files, |index, file| -> Option<FileRank> {
        let score = query.score(file)?;
        if query.is_empty() {
            return Some((Reverse(0), false, 0, index));
        }
        let name_missed = query.score(file_name(file)).is_none();
        Some((Reverse(score), name_missed, file.len(), index))
    });
    let total = matches.len();
    if total > MAX_CANDIDATES {
        matches.select_nth_unstable(MAX_CANDIDATES);
        matches.truncate(MAX_CANDIDATES);
    }
    matches.sort_unstable();
    let candidates = matches
        .into_iter()
        .map(|(.., index)| {
            let file = &files[index];
            let path = root.join(file);
            Candidate {
                matches: query
                    .score_and_ranges(file)
                    .map(|(_, ranges)| ranges)
                    .unwrap_or_default(),
                icon: Some(Icon::File),
                tag: visited.contains(path.as_path()).then_some("open"),
                ..Candidate::new(file.clone())
            }
        })
        .collect();
    Completions {
        candidates,
        total,
        ..Completions::default()
    }
}

/// Entries of the directory typed so far, matching the name being typed.
pub fn directory_candidates(input: &str) -> Completions {
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
