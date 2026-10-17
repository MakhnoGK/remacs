# 0067 — Tailwind: every server completes and documents; colors as swatches

Status: accepted, 2026-10-17

## Decision

- **Completion and hover ask every server of a buffer**, not only the first: Tailwind
  serves TSX, HTML or CSS beside the language's own server. Lists are joined in the
  servers' order once all answered, incomplete if any is; hover shows the first server's
  answer that says anything. A server that cannot complete answers with nothing at once,
  so it holds no list up. Definitions stay the first server's.
- **An item is matched against the text from its own start to point**, as VS Code does:
  Tailwind's `bg-red-500` replaces `bg-re`, while the word at point is only `re`.
- **Colors that text names come from `textDocument/documentColor`**, asked once per
  version of each file of servers with a `colorProvider` (Tailwind, CSS, HTML). They
  follow edits until the next answer; those an edit touches are dropped until then.
- **Swatches are a line under the text in its color**, not a square inserted before
  it: the column grid, cursor, mouse and horizontal scrolling stay as they are. Color
  completions (`#rrggbb` documentation) show the color in place of their kind's badge.
  `ui.toml` [swatches] `inline`, `thickness`, `completion`.
- **The catalog's Tailwind configuration names `classFunctions`** (clsx, cn, cva, cx,
  tw, twMerge, twJoin, classnames); `lsp.toml` `configuration` replaces them.

## Consequences

- Completion waits for the slowest server of the buffer.
- Inline swatches off still asks the servers for colors.
- Tailwind lists every class at once (about 23,000 for Tailwind 4); they are converted on
  the server's reader thread, and filtering them takes about a millisecond a key.

## Later

- A color picker on a swatch (`textDocument/colorPresentation`).
