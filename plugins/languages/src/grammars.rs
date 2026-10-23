use remacs_syntax::Grammar;

use crate::modes::*;

/// Our own queries, written from each grammar's node types (clean room, ADR 0081).
macro_rules! query {
    ($language:literal) => {
        include_str!(concat!("../queries/", $language, "/highlights.scm"))
    };
}

const ECMASCRIPT: &str = query!("ecmascript");

/// Dialects extend their base language's queries, which therefore come first.
pub const GRAMMARS: &[Grammar] = &[
    Grammar {
        mode: RUST,
        language: tree_sitter_rust::LANGUAGE,
        highlights: &[query!("rust")],
    },
    Grammar {
        mode: C,
        language: tree_sitter_c::LANGUAGE,
        highlights: &[query!("c")],
    },
    Grammar {
        mode: CPP,
        language: tree_sitter_cpp::LANGUAGE,
        highlights: &[query!("c"), query!("cpp")],
    },
    Grammar {
        mode: TYPESCRIPT,
        language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        highlights: &[ECMASCRIPT, query!("typescript")],
    },
    Grammar {
        mode: TSX,
        language: tree_sitter_typescript::LANGUAGE_TSX,
        highlights: &[ECMASCRIPT, query!("jsx"), query!("typescript")],
    },
    // React projects write JSX in plain `.js` files too.
    Grammar {
        mode: JAVASCRIPT,
        language: tree_sitter_javascript::LANGUAGE,
        highlights: &[ECMASCRIPT, query!("javascript"), query!("jsx")],
    },
    Grammar {
        mode: JSX,
        language: tree_sitter_javascript::LANGUAGE,
        highlights: &[ECMASCRIPT, query!("javascript"), query!("jsx")],
    },
    Grammar {
        mode: HTML,
        language: tree_sitter_html::LANGUAGE,
        highlights: &[query!("html")],
    },
    Grammar {
        mode: CSS,
        language: tree_sitter_css::LANGUAGE,
        highlights: &[query!("css")],
    },
    Grammar {
        mode: JSON,
        language: tree_sitter_json::LANGUAGE,
        highlights: &[query!("json")],
    },
    Grammar {
        mode: TOML,
        language: tree_sitter_toml_ng::LANGUAGE,
        highlights: &[query!("toml")],
    },
    Grammar {
        mode: YAML,
        language: tree_sitter_yaml::LANGUAGE,
        highlights: &[query!("yaml")],
    },
    Grammar {
        mode: XML,
        language: tree_sitter_xml::LANGUAGE_XML,
        highlights: &[query!("xml")],
    },
    // Block structure only: inline markup is a second grammar, injected into each block.
    Grammar {
        mode: MARKDOWN,
        language: tree_sitter_md::LANGUAGE,
        highlights: &[query!("markdown")],
    },
    Grammar {
        mode: BASH,
        language: tree_sitter_bash::LANGUAGE,
        highlights: &[query!("bash")],
    },
    Grammar {
        mode: MAKE,
        language: tree_sitter_make::LANGUAGE,
        highlights: &[query!("make")],
    },
    Grammar {
        mode: CMAKE,
        language: tree_sitter_cmake::LANGUAGE,
        highlights: &[query!("cmake")],
    },
    Grammar {
        mode: DIFF,
        language: tree_sitter_diff::LANGUAGE,
        highlights: &[query!("diff")],
    },
    Grammar {
        mode: SQL,
        language: tree_sitter_sequel::LANGUAGE,
        highlights: &[query!("sql")],
    },
    Grammar {
        mode: PYTHON,
        language: tree_sitter_python::LANGUAGE,
        highlights: &[query!("python")],
    },
    Grammar {
        mode: GO,
        language: tree_sitter_go::LANGUAGE,
        highlights: &[query!("go")],
    },
    Grammar {
        mode: JAVA,
        language: tree_sitter_java::LANGUAGE,
        highlights: &[query!("java")],
    },
    Grammar {
        mode: CSHARP,
        language: tree_sitter_c_sharp::LANGUAGE,
        highlights: &[query!("csharp")],
    },
    Grammar {
        mode: SWIFT,
        language: tree_sitter_swift::LANGUAGE,
        highlights: &[query!("swift")],
    },
    Grammar {
        mode: OBJC,
        language: tree_sitter_objc::LANGUAGE,
        highlights: &[query!("c"), query!("objc")],
    },
    Grammar {
        mode: DART,
        language: tree_sitter_dart::LANGUAGE,
        highlights: &[query!("dart")],
    },
    Grammar {
        mode: SCALA,
        language: tree_sitter_scala::LANGUAGE,
        highlights: &[query!("scala")],
    },
    // The full grammar, with the HTML around `<?php` tags.
    Grammar {
        mode: PHP,
        language: tree_sitter_php::LANGUAGE_PHP,
        highlights: &[query!("php")],
    },
    Grammar {
        mode: RUBY,
        language: tree_sitter_ruby::LANGUAGE,
        highlights: &[query!("ruby")],
    },
    Grammar {
        mode: LUA,
        language: tree_sitter_lua::LANGUAGE,
        highlights: &[query!("lua")],
    },
    Grammar {
        mode: ZIG,
        language: tree_sitter_zig::LANGUAGE,
        highlights: &[query!("zig")],
    },
    Grammar {
        mode: HASKELL,
        language: tree_sitter_haskell::LANGUAGE,
        highlights: &[query!("haskell")],
    },
    Grammar {
        mode: ELIXIR,
        language: tree_sitter_elixir::LANGUAGE,
        highlights: &[query!("elixir")],
    },
    Grammar {
        mode: NIX,
        language: tree_sitter_nix::LANGUAGE,
        highlights: &[query!("nix")],
    },
];
