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
const JSX_COMMENT: CommentSyntax = CommentSyntax {
    start: "{/* ",
    end: " */}",
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
        markup_comment: None,
    }
}

/// A JavaScript dialect whose markup lines comment out as JSX children.
const fn with_jsx(mode: Mode) -> Mode {
    Mode {
        markup_comment: Some(JSX_COMMENT),
        ..mode
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
    with_jsx(mode(TSX, &["tsx"], JSX_PAIRS, LINE_COMMENT)),
    // React projects write JSX in plain `.js` files too.
    with_jsx(mode(
        JAVASCRIPT,
        &["js", "mjs", "cjs"],
        JS_PAIRS,
        LINE_COMMENT,
    )),
    with_jsx(mode(JSX, &["jsx"], JSX_PAIRS, LINE_COMMENT)),
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

/// JavaScript and TypeScript modes and their LSP language ids.
const SCRIPTS: &[(&str, &str)] = &[
    (TYPESCRIPT, "typescript"),
    (TSX, "typescriptreact"),
    (JAVASCRIPT, "javascript"),
    (JSX, "javascriptreact"),
];

/// Modes whose files prettier formats.
pub const PRETTIER_MODES: &[&str] = &[TYPESCRIPT, TSX, JAVASCRIPT, JSX, HTML, CSS];

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
        configuration: None,
        pull_diagnostics: false,
        node_modules: false,
        required_files: &[],
    },
    LanguageServer {
        name: "clangd",
        command: "clangd",
        args: &["--function-arg-placeholders=0"],
        languages: &[(C, "c"), (CPP, "cpp")],
        root_markers: &["compile_commands.json", "compile_flags.txt", ".clangd"],
        initialization_options: None,
        configuration: None,
        pull_diagnostics: false,
        node_modules: false,
        required_files: &[],
    },
    // TypeScript 7's own server, native and much leaner than typescript-language-server.
    LanguageServer {
        name: "tsc",
        command: "tsc",
        args: &["--lsp", "--stdio"],
        languages: SCRIPTS,
        root_markers: &["package.json", "tsconfig.json", "jsconfig.json"],
        initialization_options: None,
        configuration: None,
        pull_diagnostics: true,
        // A project's own `tsc` is mostly TypeScript 5, which has no `--lsp`.
        node_modules: false,
        required_files: &[],
    },
    LanguageServer {
        name: "vscode-html-language-server",
        command: "vscode-html-language-server",
        args: &["--stdio"],
        languages: &[(HTML, "html")],
        root_markers: &["package.json"],
        // Formats only when asked to provide it.
        initialization_options: Some(r#"{ "provideFormatter": true }"#),
        configuration: None,
        pull_diagnostics: false,
        node_modules: false,
        required_files: &[],
    },
    LanguageServer {
        name: "vscode-css-language-server",
        command: "vscode-css-language-server",
        args: &["--stdio"],
        languages: &[(CSS, "css")],
        root_markers: &["package.json"],
        initialization_options: Some(r#"{ "provideFormatter": true }"#),
        configuration: None,
        pull_diagnostics: false,
        node_modules: false,
        required_files: &[],
    },
    // A linter beside tsc, which answers the questions; only for projects that use it.
    LanguageServer {
        name: "oxlint",
        command: "oxlint",
        args: &["--lsp"],
        languages: SCRIPTS,
        root_markers: &["package.json", ".oxlintrc.json"],
        initialization_options: None,
        configuration: None,
        pull_diagnostics: false,
        node_modules: true,
        required_files: &[
            ".oxlintrc.json",
            ".oxlintrc.jsonc",
            "node_modules/.bin/oxlint",
        ],
    },
    // Loads the project's own eslint, else the global one; only for projects with a config.
    LanguageServer {
        name: "eslint",
        command: "vscode-eslint-language-server",
        args: &["--stdio"],
        languages: SCRIPTS,
        root_markers: &["package.json"],
        initialization_options: None,
        // VS Code's defaults, which the server expects in full. `packageManager` lets it
        // find a global eslint; fixes on save are left to remacs.
        configuration: Some(
            r#"{
                "validate": "on",
                "run": "onType",
                "packageManager": "npm",
                "nodePath": null,
                "workingDirectory": { "mode": "auto" },
                "options": {},
                "experimental": {},
                "rulesCustomizations": [],
                "problems": { "shortenToSingleLine": false },
                "onIgnoredFiles": "off",
                "quiet": false,
                "format": false,
                "codeActionOnSave": { "enable": false, "mode": "all" },
                "codeAction": {
                    "disableRuleComment": {
                        "enable": true,
                        "location": "separateLine",
                        "commentStyle": "line"
                    },
                    "showDocumentation": { "enable": false }
                }
            }"#,
        ),
        pull_diagnostics: true,
        node_modules: true,
        required_files: &[
            "eslint.config.js",
            "eslint.config.mjs",
            "eslint.config.cjs",
            "eslint.config.ts",
            "eslint.config.mts",
            "eslint.config.cts",
            ".eslintrc.js",
            ".eslintrc.cjs",
            ".eslintrc.json",
            ".eslintrc.yaml",
            ".eslintrc.yml",
            ".eslintrc",
        ],
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
