# 0002 — Everything is a plugin

Status: accepted, 2026-10-04

## Decision

A thin core (buffers, windows, commands, keymaps, hooks, minibuffer); every feature is a plugin.

- **Built-in plugins** are Rust crates implementing `trait Plugin`, registered statically.
  No overhead; used for modes, syntax, LSP, Git, debugger.
- **Third-party plugins** are WASM components (wasmtime, WIT interface): sandboxed, stable ABI.
- No Rust dylib plugins: Rust has no stable ABI.

## As built (M2)

- `remacs-core`: `Editor`, `Buffer` (text + point + goal column), `Viewport`, `Keymap`
  (Emacs key notation, prefix sequences), `Commands`, `Modes`, and `Registry` that a
  `Plugin` fills in `register`.
- Commands are `fn(&mut Editor) -> CommandResult`; errors go to the echo area.
- Undo boundaries are set before each command; up to 20 consecutive self-inserts
  undo together, as in Emacs.
- Built-in plugins live in `plugins/`: `editing` (motion, editing, undo, save, quit)
  and `languages` (major modes by extension).
- The UI only translates input (Option = Meta, Command = Super) and draws; it holds no
  editing logic.

## Notes

- The WIT interface/SDK should be permissively licensed so third parties can publish
  plugins under any license.
- Initial languages: Rust, C/C++, TypeScript/JavaScript, JSX/TSX, HTML, CSS — each a
  tree-sitter grammar plus an LSP server mapping.
