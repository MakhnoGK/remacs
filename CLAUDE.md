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
- Menus follow commands: when adding, renaming or removing a command a person would look
  for, update the menu items its plugin declares (`MenuItem` in `Registry::menu_items`,
  with `enabled_when` if it cannot always run); a new group or menu goes into the `frame`
  plugin's layout (ADR 0060). The test in `crates/app/src/menu_bar.rs` checks that every
  item runs an existing command.
- Performance changes are always measured: benchmark first (divan with its allocation
  profiler in the crate's `benches/`, dhat for peak heap, or a test counting calls or
  frames), record the baseline, change the code, measure again with the same benchmark.
  Keep a change only if it gains at least 10% in time, allocations or peak heap;
  otherwise revert it. The benchmark stays in the repo.

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
