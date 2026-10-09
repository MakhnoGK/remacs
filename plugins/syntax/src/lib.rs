//! Built-in plugin: tree-sitter parsing on a background thread and syntax highlighting.

mod buffer_syntax;
mod highlighter;
mod matching_pair;
mod parser_thread;
mod snippet;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use remacs_core::{Plugin, Registry, Section};
use serde::Deserialize;
pub use tree_sitter_language::LanguageFn;

/// `syntax.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct SyntaxSettings {
    /// Whether the bracket or tag at point is highlighted with the one it pairs with.
    pub matching_pairs: bool,
}

impl Default for SyntaxSettings {
    fn default() -> Self {
        Self {
            matching_pairs: true,
        }
    }
}

impl Section for SyntaxSettings {
    const NAME: &str = "syntax";
}

/// A tree-sitter grammar and its highlight queries, for one major mode.
pub struct Grammar {
    pub mode: &'static str,
    pub language: LanguageFn,
    /// Queries joined in order; when several patterns capture one node, the last wins.
    pub highlights: &'static [&'static str],
}

pub struct SyntaxPlugin {
    grammars: &'static [Grammar],
}

impl SyntaxPlugin {
    pub const fn new(grammars: &'static [Grammar]) -> Self {
        Self { grammars }
    }
}

impl Plugin for SyntaxPlugin {
    fn register(&self, registry: &mut Registry) {
        let grammars = self.grammars;
        registry
            .post_command_hooks
            .push(Rc::new(move |editor| buffer_syntax::sync(editor, grammars)));
        registry.decorators.push(Rc::new(buffer_syntax::highlight));
        let matching_pairs = Rc::new(Cell::new(SyntaxSettings::default().matching_pairs));
        registry.settings_hooks.push(Rc::new({
            let matching_pairs = Rc::clone(&matching_pairs);
            move |editor| {
                let settings = editor.settings_section::<SyntaxSettings>();
                matching_pairs.set(settings.matching_pairs);
            }
        }));
        registry.decorators.push(Rc::new(move |buffer, range| {
            if matching_pairs.get() {
                matching_pair::highlight(buffer, range)
            } else {
                Vec::new()
            }
        }));
        registry
            .syntax_probes
            .push(Rc::new(buffer_syntax::context_at));
        let snippets = RefCell::new(snippet::SnippetHighlighter::new(grammars));
        registry.code_highlighters.push(Rc::new(move |mode, code| {
            snippets.borrow_mut().highlight(mode, code)
        }));
    }
}

#[cfg(test)]
mod tests;
