use remacs_syntax::Grammar;

use crate::modes::*;

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
    Grammar {
        mode: JSON,
        language: tree_sitter_json::LANGUAGE,
        highlights: &[tree_sitter_json::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: TOML,
        language: tree_sitter_toml_ng::LANGUAGE,
        highlights: &[tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: YAML,
        language: tree_sitter_yaml::LANGUAGE,
        highlights: &[tree_sitter_yaml::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: XML,
        language: tree_sitter_xml::LANGUAGE_XML,
        highlights: &[tree_sitter_xml::XML_HIGHLIGHT_QUERY],
    },
    // Block structure only: inline markup is a second grammar, injected into each block.
    Grammar {
        mode: MARKDOWN,
        language: tree_sitter_md::LANGUAGE,
        highlights: &[tree_sitter_md::HIGHLIGHT_QUERY_BLOCK],
    },
    Grammar {
        mode: BASH,
        language: tree_sitter_bash::LANGUAGE,
        highlights: &[tree_sitter_bash::HIGHLIGHT_QUERY],
    },
    Grammar {
        mode: MAKE,
        language: tree_sitter_make::LANGUAGE,
        highlights: &[tree_sitter_make::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: CMAKE,
        language: tree_sitter_cmake::LANGUAGE,
        highlights: &[tree_sitter_cmake::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: DIFF,
        language: tree_sitter_diff::LANGUAGE,
        highlights: &[tree_sitter_diff::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: SQL,
        language: tree_sitter_sequel::LANGUAGE,
        highlights: &[tree_sitter_sequel::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: PYTHON,
        language: tree_sitter_python::LANGUAGE,
        highlights: &[tree_sitter_python::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: GO,
        language: tree_sitter_go::LANGUAGE,
        highlights: &[tree_sitter_go::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: JAVA,
        language: tree_sitter_java::LANGUAGE,
        highlights: &[tree_sitter_java::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: CSHARP,
        language: tree_sitter_c_sharp::LANGUAGE,
        highlights: &[tree_sitter_c_sharp::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: SWIFT,
        language: tree_sitter_swift::LANGUAGE,
        highlights: &[tree_sitter_swift::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: OBJC,
        language: tree_sitter_objc::LANGUAGE,
        highlights: &[tree_sitter_objc::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: DART,
        language: tree_sitter_dart::LANGUAGE,
        highlights: &[tree_sitter_dart::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: SCALA,
        language: tree_sitter_scala::LANGUAGE,
        highlights: &[tree_sitter_scala::HIGHLIGHTS_QUERY],
    },
    // The full grammar, with the HTML around `<?php` tags.
    Grammar {
        mode: PHP,
        language: tree_sitter_php::LANGUAGE_PHP,
        highlights: &[tree_sitter_php::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: RUBY,
        language: tree_sitter_ruby::LANGUAGE,
        highlights: &[tree_sitter_ruby::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: LUA,
        language: tree_sitter_lua::LANGUAGE,
        highlights: &[tree_sitter_lua::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: ZIG,
        language: tree_sitter_zig::LANGUAGE,
        highlights: &[tree_sitter_zig::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: HASKELL,
        language: tree_sitter_haskell::LANGUAGE,
        highlights: &[tree_sitter_haskell::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: ELIXIR,
        language: tree_sitter_elixir::LANGUAGE,
        highlights: &[tree_sitter_elixir::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: NIX,
        language: tree_sitter_nix::LANGUAGE,
        highlights: &[tree_sitter_nix::HIGHLIGHTS_QUERY],
    },
];
