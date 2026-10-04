//! Built-in plugin: major modes for the initial languages, chosen by file extension, and
//! their tree-sitter grammars.

use remacs_core::{Mode, Plugin, Registry};
use remacs_syntax::Grammar;

pub struct LanguagesPlugin;

const RUST: &str = "Rust";
const C: &str = "C";
const CPP: &str = "C++";
const TYPESCRIPT: &str = "TypeScript";
const TSX: &str = "TSX";
const JAVASCRIPT: &str = "JavaScript";
const JSX: &str = "JSX";
const HTML: &str = "HTML";
const CSS: &str = "CSS";

const MODES: &[(&str, &[&str])] = &[
    (RUST, &["rs"]),
    (C, &["c", "h"]),
    (CPP, &["cc", "cpp", "cxx", "hh", "hpp", "hxx"]),
    (TYPESCRIPT, &["ts", "mts", "cts"]),
    (TSX, &["tsx"]),
    (JAVASCRIPT, &["js", "mjs", "cjs"]),
    (JSX, &["jsx"]),
    (HTML, &["html", "htm"]),
    (CSS, &["css"]),
];

/// Upstream's all-caps constant pattern never matches (its regex ends in a stray quote).
const RUST_CONSTANTS: &str = r#"((identifier) @constant (#match? @constant "^[A-Z][A-Z\\d_]+$"))"#;

/// Dialects extend their base language's queries, which therefore come first.
pub const GRAMMARS: &[Grammar] = &[
    Grammar {
        mode: RUST,
        language: tree_sitter_rust::LANGUAGE,
        highlights: &[tree_sitter_rust::HIGHLIGHTS_QUERY, RUST_CONSTANTS],
    },
    Grammar {
        mode: C,
        language: tree_sitter_c::LANGUAGE,
        highlights: &[tree_sitter_c::HIGHLIGHT_QUERY],
    },
    Grammar {
        mode: CPP,
        language: tree_sitter_cpp::LANGUAGE,
        highlights: &[
            tree_sitter_c::HIGHLIGHT_QUERY,
            tree_sitter_cpp::HIGHLIGHT_QUERY,
        ],
    },
    Grammar {
        mode: TYPESCRIPT,
        language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        highlights: &[
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
        ],
    },
    Grammar {
        mode: TSX,
        language: tree_sitter_typescript::LANGUAGE_TSX,
        highlights: &[
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
        ],
    },
    Grammar {
        mode: JAVASCRIPT,
        language: tree_sitter_javascript::LANGUAGE,
        highlights: &[tree_sitter_javascript::HIGHLIGHT_QUERY],
    },
    Grammar {
        mode: JSX,
        language: tree_sitter_javascript::LANGUAGE,
        highlights: &[
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
        ],
    },
    Grammar {
        mode: HTML,
        language: tree_sitter_html::LANGUAGE,
        highlights: &[tree_sitter_html::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: CSS,
        language: tree_sitter_css::LANGUAGE,
        highlights: &[tree_sitter_css::HIGHLIGHTS_QUERY],
    },
];

impl Plugin for LanguagesPlugin {
    fn register(&self, registry: &mut Registry) {
        for &(name, extensions) in MODES {
            registry.modes.add(Mode { name, extensions });
        }
    }
}

#[cfg(test)]
mod tests {
    use tree_sitter::{Language, Query};

    use super::*;

    #[test]
    fn every_mode_has_a_grammar_whose_queries_compile() {
        for &(mode, _) in MODES {
            let grammar = GRAMMARS
                .iter()
                .find(|grammar| grammar.mode == mode)
                .unwrap();
            let language = Language::new(grammar.language);
            Query::new(&language, &grammar.highlights.concat())
                .unwrap_or_else(|error| panic!("{mode}: {error}"));
        }
    }
}
