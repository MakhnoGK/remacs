# remacs

Emacs-inspired IDE from scratch in Rust + gpui. Everything is a plugin.

## Code guidelines

- Concise code, minimal comments: only the non-obvious "why", never the "what".
- Separated responsibilities; DRY and SOLID.
- Readable, descriptive names for variables, types and functions.
- Everything tunable goes into config, not code constants: colors, sizes, timings, limits,
  key bindings, behavior toggles. When adding or changing a feature, expose its settings
  and update the default config in `~/.config/remacs/config/` (`ui.toml`, `lsp.toml`,
  `format.toml`, or a new file) to match.

## Fixed decisions

- Platform: macOS only.
- Third-party plugins: WASM (wasmtime). Built-in plugins: Rust crates, registered statically.
- License: Apache-2.0 + Commons Clause (open source, no paid redistribution by third parties).
- Priorities: latency, memory, files measured in gigabytes.
- Initial languages (tree-sitter + LSP): Rust, C/C++, TypeScript/JavaScript, JSX/TSX, HTML, CSS.

## Workflow

- Commits: Conventional Commits, subject line only.
- After every completed action, re-index the repo with Cerebrium `code_index`.
- The installed app (`~/Applications/Remacs.app`, `remacs` on the command line) is a
  snapshot: update it only when asked, with `cargo xtask install`; `cargo xtask rollback`
  goes back (ADR 0056).

ADRs live in `docs/adr/`.
