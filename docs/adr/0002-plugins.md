# 0002 — Everything is a plugin

Status: accepted, 2026-10-04

## Decision

A thin core (buffers, windows, commands, keymaps, hooks, minibuffer); every feature is a plugin.

- **Built-in plugins** are Rust crates implementing `trait Plugin`, registered statically.
  No overhead; used for modes, syntax, LSP, Git, debugger.
- **Third-party plugins** are WASM components (wasmtime, WIT interface): sandboxed, stable ABI.
- No Rust dylib plugins: Rust has no stable ABI.

## Notes

- The WIT interface/SDK should be permissively licensed so third parties can publish
  plugins under any license.
- Initial languages: Rust, C/C++, TypeScript/JavaScript, JSX/TSX, HTML, CSS — each a
  tree-sitter grammar plus an LSP server mapping.
