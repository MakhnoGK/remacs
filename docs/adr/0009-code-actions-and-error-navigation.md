# 0009 — Code actions and error navigation

Status: accepted, 2026-10-04 (M4, step 5)

## Decision

- **Error navigation while editing**: `M-g n`/`M-g p` (`next-error`/`previous-error`) move
  point to the next or previous error or warning, wrapping around, and echo its message;
  infos are skipped. `M-n`/`M-p` (all problems) and the Problems list stay as they are.
- **A plugin like the others, fed by `lsp`.** `actions` owns `C-c C-a` (region, else
  point) and its popup, and applies the edits; it knows nothing of LSP. Registered before
  `lsp`, which takes its requests in the post-command hook (ADR 0007).
- **Popup at point**, like completion: the server's order with preferred actions first,
  the kind beside each title (`quick fix`, `extract`, `rewrite`…). `C-n`/`C-p`, `RET`
  applies, `C-g`/`ESC` or any other command closes it. Disabled actions are left out.
- **Carrying one out**: an action's edit is applied, then its command run
  (`workspace/executeCommand`); edits the server then asks for (`workspace/applyEdit`)
  are applied and answered from the UI thread. An action without an edit, or that keeps
  what it is computed from in `data`, is resolved first (`codeAction/resolve`; how
  rust-analyzer serves assists).
- **Problems are passed back** in the request's context, as the server reported them,
  only while the server has the text they were reported for: servers like clangd match
  them by position and message to offer their fixes.
- **Edits across files**: files not visited are opened without showing them
  (`Editor::find_file_noselect`) and left unsaved; the echo area names them. Each buffer
  gets one undo group; point stays on its character. Positions are converted against the
  buffer visiting the file, else the file.
- **Answers after another command are refused**, lists and edits alike, as with
  definitions. rust-analyzer answers out of order, so `lsp` keeps the newest list by id.
- **Capabilities**: code action literals, `isPreferred`, `data`, resolve of `edit`,
  `applyEdit` and `documentChanges`. Creating, renaming or deleting files is not supported
  and says so.

## Core additions

- `Editor::find_file_noselect`.

## Open

- No lightbulb or actions on idle; asked for only.
- A `ContentModified` failure of a resolve is shown, not retried.
