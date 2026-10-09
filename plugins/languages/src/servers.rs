use remacs_lsp::LanguageServer;

use crate::modes::*;

/// JavaScript and TypeScript modes and their LSP language ids.
const SCRIPTS: &[(&str, &str)] = &[
    (TYPESCRIPT, "typescript"),
    (TSX, "typescriptreact"),
    (JAVASCRIPT, "javascript"),
    (JSX, "javascriptreact"),
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
        configuration: None,
        pull_diagnostics: false,
        reports_loading: true,
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
        reports_loading: false,
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
        reports_loading: false,
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
        reports_loading: false,
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
        reports_loading: false,
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
        reports_loading: false,
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
        reports_loading: false,
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
