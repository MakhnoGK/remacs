# remacs

Emacs-inspired IDE from scratch in Rust + gpui. Everything is a plugin.

## Code guidelines

- Concise code, minimal comments: only the non-obvious "why", never the "what".
- Separated responsibilities; DRY and SOLID.
- Readable, descriptive names for variables, types and functions.

## Fixed decisions

- Platform: macOS only.
- Third-party plugins: WASM (wasmtime). Built-in plugins: Rust crates, registered statically.
- License: Apache-2.0 + Commons Clause (open source, no paid redistribution by third parties).
- Priorities: latency, memory, files measured in gigabytes.
- Initial languages (tree-sitter + LSP): Rust, C/C++, TypeScript/JavaScript, JSX/TSX, HTML, CSS.

ADRs live in `docs/adr/`.
