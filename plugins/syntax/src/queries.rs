//! The user's own highlight queries, `queries/<mode>/highlights.scm` in remacs's home:
//! with `; extends` on the first line they follow remacs's patterns, so theirs win;
//! otherwise they take their place.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use remacs_core::{CommandResult, Editor};
use tree_sitter::Language;

use crate::Grammar;
use crate::highlighter::Highlighter;

pub(crate) const RELOAD: &str = "reload-syntax-queries";

/// Each mode's whole highlight query where the user has one.
pub(crate) type UserQueries = Arc<HashMap<&'static str, String>>;

#[derive(Default)]
pub(crate) struct Queries {
    dir: Option<PathBuf>,
    grammars: &'static [Grammar],
    pub highlights: UserQueries,
}

/// Reads the user's queries under `home`, now and on `reload-syntax-queries`.
pub fn keep_queries_in(editor: &mut Editor, home: &Path) {
    editor.plugin_state::<Queries>().dir = Some(home.join("queries"));
    load(editor);
}

pub(crate) fn know_grammars(editor: &mut Editor, grammars: &'static [Grammar]) {
    editor.plugin_state::<Queries>().grammars = grammars;
}

pub(crate) fn reload(editor: &mut Editor) -> CommandResult {
    if let Some(count) = load(editor) {
        editor.set_message(format!("{count} queries of one's own"));
    }
    Ok(())
}

/// Reads them again, and how many there are; one that does not compile is named, and
/// remacs's stays in use.
fn load(editor: &mut Editor) -> Option<usize> {
    let state = editor.plugin_state::<Queries>();
    let (Some(dir), grammars) = (state.dir.clone(), state.grammars) else {
        return Some(0);
    };
    let mut highlights = HashMap::new();
    let mut errors = Vec::new();
    for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Finder's `.DS_Store` and the like are no modes.
        if name.starts_with('.') || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Some(grammar) =
            (grammars.iter()).find(|grammar| grammar.mode.eq_ignore_ascii_case(&name))
        else {
            errors.push(format!("queries/{name}: no mode is called {name}"));
            continue;
        };
        let Ok(own) = fs::read_to_string(entry.path().join("highlights.scm")) else {
            continue;
        };
        let extends = own
            .lines()
            .next()
            .is_some_and(|first| first.trim_start_matches(';').trim() == "extends");
        let source = if extends {
            grammar.highlights.concat() + "\n" + &own
        } else {
            own
        };
        match Highlighter::new(&Language::new(grammar.language), &[&source]) {
            Ok(_) => {
                highlights.insert(grammar.mode, source);
            }
            Err(error) => errors.push(format!("queries/{name}/highlights.scm: {error}")),
        }
    }
    let count = highlights.len();
    editor.plugin_state::<Queries>().highlights = Arc::new(highlights);
    match errors.into_iter().next() {
        Some(error) => {
            editor.set_error(error);
            None
        }
        None => Some(count),
    }
}
