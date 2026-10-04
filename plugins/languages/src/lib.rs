//! Built-in plugin: major modes for the initial languages, chosen by file extension, their
//! tree-sitter grammars and language servers.

use remacs_core::{CommentSyntax, Mode, Plugin, Registry};
use remacs_lsp::LanguageServer;
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

/// `'` starts lifetimes in Rust, so only `"` pairs there.
const RUST_PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"')];
const C_PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')];
const JS_PAIRS: &[(char, char)] = &[
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('"', '"'),
    ('\'', '\''),
    ('`', '`'),
];
/// `<` pairs only where it opens tags: elsewhere it is also less-than.
const JSX_PAIRS: &[(char, char)] = &[
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('<', '>'),
    ('"', '"'),
    ('\'', '\''),
    ('`', '`'),
];
const HTML_PAIRS: &[(char, char)] = &[
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('<', '>'),
    ('"', '"'),
    ('\'', '\''),
];

const LINE_COMMENT: CommentSyntax = CommentSyntax {
    start: "// ",
    end: "",
};
const BLOCK_COMMENT: CommentSyntax = CommentSyntax {
    start: "/* ",
    end: " */",
};
const HTML_COMMENT: CommentSyntax = CommentSyntax {
    start: "<!-- ",
    end: " -->",
};

const fn mode(
    name: &'static str,
    extensions: &'static [&'static str],
    pairs: &'static [(char, char)],
    comment: CommentSyntax,
) -> Mode {
    Mode {
        name,
        extensions,
        pairs,
        comment: Some(comment),
    }
}

const MODES: &[Mode] = &[
    mode(RUST, &["rs"], RUST_PAIRS, LINE_COMMENT),
    mode(C, &["c", "h"], C_PAIRS, LINE_COMMENT),
    mode(
        CPP,
        &["cc", "cpp", "cxx", "hh", "hpp", "hxx"],
        C_PAIRS,
        LINE_COMMENT,
    ),
    mode(TYPESCRIPT, &["ts", "mts", "cts"], JS_PAIRS, LINE_COMMENT),
    mode(TSX, &["tsx"], JSX_PAIRS, LINE_COMMENT),
    mode(JAVASCRIPT, &["js", "mjs", "cjs"], JS_PAIRS, LINE_COMMENT),
    mode(JSX, &["jsx"], JSX_PAIRS, LINE_COMMENT),
    mode(HTML, &["html", "htm"], HTML_PAIRS, HTML_COMMENT),
    mode(CSS, &["css"], C_PAIRS, BLOCK_COMMENT),
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

pub const SERVERS: &[LanguageServer] = &[
    LanguageServer {
        name: "rust-analyzer",
        command: "rust-analyzer",
        args: &[],
        languages: &[(RUST, "rust")],
        root_markers: &["Cargo.toml"],
        // Calls complete to `f()` with point inside, not to argument names to type over.
        initialization_options: Some(
            r#"{ "completion": { "callable": { "snippets": "add_parentheses" } } }"#,
        ),
        pull_diagnostics: false,
    },
    LanguageServer {
        name: "clangd",
        command: "clangd",
        args: &["--function-arg-placeholders=0"],
        languages: &[(C, "c"), (CPP, "cpp")],
        root_markers: &["compile_commands.json", "compile_flags.txt", ".clangd"],
        initialization_options: None,
        pull_diagnostics: false,
    },
    // TypeScript 7's own server, native and much leaner than typescript-language-server.
    LanguageServer {
        name: "tsc",
        command: "tsc",
        args: &["--lsp", "--stdio"],
        languages: &[
            (TYPESCRIPT, "typescript"),
            (TSX, "typescriptreact"),
            (JAVASCRIPT, "javascript"),
            (JSX, "javascriptreact"),
        ],
        root_markers: &["package.json", "tsconfig.json", "jsconfig.json"],
        initialization_options: None,
        pull_diagnostics: true,
    },
    LanguageServer {
        name: "vscode-html-language-server",
        command: "vscode-html-language-server",
        args: &["--stdio"],
        languages: &[(HTML, "html")],
        root_markers: &["package.json"],
        initialization_options: None,
        pull_diagnostics: false,
    },
    LanguageServer {
        name: "vscode-css-language-server",
        command: "vscode-css-language-server",
        args: &["--stdio"],
        languages: &[(CSS, "css")],
        root_markers: &["package.json"],
        initialization_options: None,
        pull_diagnostics: false,
    },
];

impl Plugin for LanguagesPlugin {
    fn register(&self, registry: &mut Registry) {
        for &mode in MODES {
            registry.modes.add(mode);
        }
    }
}

#[cfg(test)]
mod tests {
    use tree_sitter::{Language, Query};

    use super::*;

    #[test]
    fn every_mode_has_a_grammar_whose_queries_compile() {
        for &Mode { name: mode, .. } in MODES {
            let grammar = GRAMMARS
                .iter()
                .find(|grammar| grammar.mode == mode)
                .unwrap();
            let language = Language::new(grammar.language);
            Query::new(&language, &grammar.highlights.concat())
                .unwrap_or_else(|error| panic!("{mode}: {error}"));
        }
    }

    #[test]
    fn servers_serve_known_modes() {
        for server in SERVERS {
            for (mode, _) in server.languages {
                assert!(MODES.iter().any(|known| known.name == *mode), "{mode}");
            }
        }
    }
}
