//! Built-in plugin: tree-sitter parsing on a background thread and syntax highlighting.

mod buffer_syntax;
mod highlighter;
mod parser_thread;

use std::rc::Rc;

use remacs_core::{Plugin, Registry};
pub use tree_sitter_language::LanguageFn;

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
    }
}

#[cfg(test)]
mod tests;
