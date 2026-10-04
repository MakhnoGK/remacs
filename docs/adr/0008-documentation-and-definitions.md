# 0008 — Documentation at point and definitions

Status: accepted, 2026-10-04 (M4, step 4)

## Decision

- **Two plugins, like eldoc and xref, fed by `lsp`.** `documentation` owns `C-h .` and its
  popup, `xref` owns `M-.`/`M-,`; neither knows LSP. Both queue requests that `lsp` takes
  in its post-command hook and answers with `deliver`, as completion does (ADR 0007), so
  both are registered before `lsp`.
- **Asked for, not on idle.** `C-h .` shows the summary: the blocks before the first
  heading past the start (`# Examples`). `C-h .` again shows all of it. Any other command,
  `ESC` or point moved by the mouse hides it.
- **Board 03 layout**: the popup's text lines up with the symbol, below the line, or above
  when there is more room; header (container left, kind right), signature box, body, keys.
  The symbol the server names (`Hover.range`) is highlighted while shown (`Face::Highlight`).
- **Hover conversion** is a heuristic over the server's Markdown: a leading one-line code
  block before another is the container (rust-analyzer's module), `impl …` and clangd's
  `// In file.h` lines before a declaration join it; TypeScript's `(method) …` and clangd's
  heading `function `strlen`` give the kind, else the declaration's first keyword does.
- **Definitions**: answers that come after another command are dropped, so point never
  jumps away from what the user went on to do. One location is visited and centered if
  off screen; several are listed in the minibuffer with their lines. `M-,` returns to
  where the lookup started (16 places).
- **Positions** in other files are converted on the UI thread against the buffer visiting
  the file, else the file on disk, since the server sees that text.
- **Failures answer with nothing.** A failed hover or definition request, or a buffer
  without a server, gives "No documentation at point" / "No definitions found".
- `lsp` now hands answers over before syncing buffers, so a file a definition opened is
  attached in the same pass.
- **Code in documentation is highlighted like buffers.** `syntax` registers a code
  highlighter that parses a snippet whole with the mode's grammar, on the UI thread; the
  view keeps the faces per snippet. `lsp` tags each `Code` with a mode: a fence's tag
  matched against the server's language ids, else the document's mode, which the
  declaration is written in too.
- **Documentation scrolls**, in the hover popup and beside completions: the mouse wheel
  over it (popups take the mouse from the buffer behind them), and `C-M-v`/`C-M-V` a body
  height at a time, like `scroll-other-window`. Plugins count pages (`Popup::
  documentation_pages`); the view owns the pixel offset, which starts at the top for
  other documentation. Scrolling the summary shows all of it.

## Core additions

- `Popup` without rows: documentation alone. `Documentation::{container, kind}` replace
  `title`; `DocBlock::Heading`.
- `Face::Highlight`.
- `Code { text, mode }` for the signature and code blocks; `Registry::code_highlighters`,
  `Editor::highlight_code`; `Popup::documentation_pages`.

## Open

- No `M-?` (references), no peek of a definition without visiting it.
