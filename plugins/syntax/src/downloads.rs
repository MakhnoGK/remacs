//! Grammars remacs does not build in, downloaded from its own releases the first time a
//! file wants one, and loaded from where the tools plugin keeps them (ADR 0087).

use std::collections::HashSet;

use remacs_core::Editor;
use remacs_tools::{Source as Origin, Tool, ToolKind};

use crate::{Grammar, Source, wasm};

const REPOSITORY: &str = "MakhnoGK/remacs";
const TAG_PREFIX: &str = "grammars-";

/// The downloads of `grammars`, kept for the run as tools are.
pub(crate) fn tools(grammars: &'static [Grammar]) -> Vec<&'static Tool> {
    grammars
        .iter()
        .filter_map(|grammar| match grammar.language {
            Source::Downloaded { name, release } => {
                let file: &'static str = format!("tree-sitter-{name}.wasm").leak();
                Some(&*Box::leak(Box::new(Tool {
                    name: &file[..file.len() - ".wasm".len()],
                    kind: ToolKind::Grammar,
                    source: Origin::GitHub {
                        repository: REPOSITORY,
                        tag_prefix: TAG_PREFIX,
                        asset: file,
                    },
                    program: file,
                    arch: None,
                    needs: &[],
                    version: Some(release),
                })))
            }
            Source::Built(_) => None,
        })
        .collect()
}

/// Downloaded grammars asked for in this run, so that each is fetched or loaded once.
#[derive(Default)]
struct Asked(HashSet<&'static str>);

/// Loads the window buffer's grammar if it is downloaded and there, else downloads it if
/// `download`; highlighting follows once it is loaded.
pub(crate) fn want(editor: &mut Editor, grammars: &'static [Grammar], download: bool) {
    let mode = editor.window_buffer().mode();
    let Some(&Grammar { language: Source::Downloaded { name, .. }, mode, .. }) =
        grammars.iter().find(|grammar| grammar.mode == mode)
    else {
        return;
    };
    if wasm::loaded(name).is_some() || editor.plugin_state::<Asked>().0.contains(name) {
        return;
    }
    let tool = format!("tree-sitter-{name}");
    let load = move |editor: &mut Editor, path: &std::path::Path| match wasm::load(name, path) {
        Ok(()) => (editor.notifier())(),
        Err(error) => editor.set_error(format!("The {mode} grammar: {error}")),
    };
    if let Some(program) = remacs_tools::installed(editor, &tool) {
        editor.plugin_state::<Asked>().0.insert(name);
        load(editor, &program.path);
    } else if download {
        editor.plugin_state::<Asked>().0.insert(name);
        remacs_tools::ensure(editor, &tool, move |editor, result| match result {
            Ok(path) => load(editor, &path),
            Err(error) => {
                editor.set_error(format!("Could not download the {mode} grammar: {error}"))
            }
        });
    }
}
