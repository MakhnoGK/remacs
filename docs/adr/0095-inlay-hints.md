# 0095 — Inlay hints

Status: accepted, 2026-11-09 (issue #204)

Servers know the types of bindings, the names of the parameters a call's arguments go
to, the type each step of a chain gives; editors show them in the code, dim, as inlay
hints (`textDocument/inlayHint`).

## Decision

- **Text between the characters, not in the buffer**: core's `InlayHint` is an offset of
  the window buffer and a text to draw before it, given by `inlay_hints` providers like
  decorators. The text view puts the hints into a line's display text once it is cut to
  the columns in view: a character's cell starts after the hints before it and a range
  of the buffer ends before them, so faces, the region and the cursor never land on a
  hint, and a click on one puts point on the character it goes before. Point, copying,
  search and every command see only the buffer. Hints are drawn in the faint color.
- **The lines in view, with as many more on each side**: the window buffer's documents
  ask for the hints of that range once per version they send, again when the server asks
  (`workspace/inlayHint/refresh`), when the settings or the toggle change, and when the
  lines in view leave the range asked. Hints an edit touches are dropped until the answer
  comes, those after it move with it, as semantic tokens' (ADR 0088). Scrolling with the
  mouse runs no command; lines drawn outside the range asked wake the editor once, to ask
  for them. Superseded requests in a batch are not sent.
- **Settings**: `inlay-hints` in `lsp.toml`, and `[inlay-hint-kinds]`, the kinds servers
  tell apart: `types` (of bindings, chains, closures' returns), `parameters`, `other`
  (rust-analyzer's lifetimes and closing braces' names). `lsp-toggle-inlay-hints`, in
  View ▸ Hide/Show Inlay Hints, flips them until remacs quits.
- **What each server offers is its own setting**, in the catalog: rust-analyzer adds the
  return types of closures with a block and the lifetimes of signatures that elide more
  than one; typescript-language-server and Vue's are asked for parameter names, variable
  and return types (tsserver hints nothing by default); gopls for the types of `:=` and
  `range` and parameter names. clangd hints by default. `[servers.<name>]` in `lsp.toml`
  changes them.

## Measured

`cargo bench -p remacs-app --bench display_lines`, a frame of 60 lines of 90 characters,
medians: 11.3 µs without hints; with three hints on every line, 36.6 µs copying the text
character by character, 20.4 µs copying it in runs between hints. Lines without hints
cost nothing more.

## Later

- `inlayHint/resolve`: a hint's tooltip and the definitions its label parts lead to,
  once the text has a mouse hover to show them on.
- Windows other than the selected one show the hints asked while their buffer was
  selected; a second window on the same buffer, scrolled elsewhere, shows none there.
- TypeScript 7's `tsc --lsp` is left to its defaults; how it takes inlay preferences is
  not settled yet.
- Lines are cut to the window's width before hints widen them, so one ending past the
  right edge only by its hints shows no fade there.
