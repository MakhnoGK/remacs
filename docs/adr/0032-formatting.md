# 0032 — Formatting

Status: accepted, 2026-10-04 (lint and format, step 4)

## Decision

- **A plugin like the others, with backends.** `format` owns `C-c C-f` (`format`: the
  region, else the buffer) and `format-buffer`, and applies the edits; it knows nothing
  of formatters. Backends take its requests in their post-command hooks, in plugin
  order: `prettier`, then `lsp`, which takes what is left and answers "No formatter for
  this buffer" without a server.
- **Minimal edits**: a backend that gets back a whole text, like prettier, turns it into
  edits with `format::edits` (imara-diff: lines, then the characters of changed lines up
  to 64 KiB), so marks and diagnostics stay and the language servers get a small change.
  Point stays on its character, also when space is inserted right before it; the edits
  undo as one step.
- **Edits for a changed buffer are dropped** ("the buffer changed meanwhile"); the
  request's snapshot must still be the buffer's text.
- **prettier for projects that use it**: a configuration file, a `prettier` key in
  `package.json` or `node_modules/prettier`, looked for up to the Git work tree, for the
  TypeScript, JavaScript, TSX, JSX, HTML and CSS modes. One Node process
  (`node --input-type=module -e worker.mjs`), started on the first format, keeps each
  project's prettier loaded: the project's own first, else the global one (`npm root
  -g`). It honours the nearest `.prettierignore` and `.editorconfig`. Plugins named in
  a config are resolved from the config file: prettier itself looks in the worker's
  working directory, wherever remacs was started. A warm format of a
  small file takes under a millisecond against about 240 ms for the CLI; a dense
  1000-line file about 160 ms, prettier's own time. Texts are copied and diffed off the
  UI thread.
- **Language servers format the rest** (`textDocument/formatting`, `rangeFormatting`),
  the buffer's first server: rust-analyzer (rustfmt and `rustfmt.toml`), clangd
  (`.clang-format`), tsc, and the CSS and HTML servers (`provideFormatter`). Options are
  4 spaces and a final newline; the formatters' own configuration decides the rest. A
  server that cannot format a region says so.

## Open

- Formatting on save: ADR 0033.
- Answers reach the buffer only while it is the window buffer's, as other server answers.
