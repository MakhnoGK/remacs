use std::collections::HashMap;

use remacs_core::Span;
use remacs_text::Text;
use tree_sitter::Parser;

use crate::highlighter::Highlighter;
use crate::parser_thread::parse;
use crate::{Grammar, wasm};

/// Highlights code that is not in a buffer, e.g. a signature in documentation. Snippets are
/// small, so they are parsed whole, on the UI thread.
pub(crate) struct SnippetHighlighter {
    grammars: &'static [Grammar],
    parser: Parser,
    /// Compiled once per mode; `None` when the queries do not compile.
    highlighters: HashMap<&'static str, Option<Highlighter>>,
}

impl SnippetHighlighter {
    pub fn new(grammars: &'static [Grammar]) -> Self {
        Self {
            grammars,
            parser: Parser::new(),
            highlighters: HashMap::new(),
        }
    }

    pub fn highlight(&mut self, mode: &'static str, code: &str, level: u8) -> Option<Vec<Span>> {
        let grammar = self.grammars.iter().find(|grammar| grammar.mode == mode)?;
        let language = grammar.language()?;
        let highlighter = self
            .highlighters
            .entry(mode)
            .or_insert_with(|| Highlighter::new(&language, grammar.highlights).ok())
            .as_ref()?;
        wasm::set_language(&mut self.parser, &language).then_some(())?;
        let text = Text::from(code).snapshot();
        let tree = parse(&mut self.parser, &text, None)?;
        Some(highlighter.spans(&tree, &text, 0..text.len(), level))
    }
}
