# 0006 — Language servers and diagnostics

Status: accepted, 2026-10-04 (M4, step 2)

## Decision

- **Two plugins, like eglot and flymake.** `diagnostics` owns a buffer's problems
  (underlines, status pill counts, the problem list, `M-n`/`M-p`) and knows nothing of
  LSP; `lsp` is one backend that feeds it through `diagnostics::publish`.
- **Diagnostics follow edits.** They are stored with the snapshot their ranges refer to and
  rebased through `Snapshot::edit_from`, so they stay on their text until the next report.
- **One server per server kind and project root.** The root is the outermost directory with
  a root marker inside the Git work tree, so a Cargo workspace or monorepo gets one process.
  A server that fails to start is not tried again.
- **Two threads per server.** The writer holds the queue until `initialize` is answered
  (position encoding and sync kind are known only then) and converts edits to LSP positions
  off the UI thread; the reader answers server requests and collects diagnostics.
- **Sync without an edit log**: each command's snapshot diff becomes one incremental
  `didChange`; a buffer turning unmodified is a save (`didSave`); dropping the buffer-local
  document, e.g. with the buffer, sends `didClose`. Files over 16 MiB are not synced.
- **Positions**: UTF-8 when the server agrees, else UTF-16.
- **Versions**: sent snapshots are kept until diagnostics for their version arrive, so
  late diagnostics map onto the text they were computed for; stale ones are dropped.
- **Push by default, pull where needed.** Servers that publish on their own are never
  asked, which would duplicate their diagnostics (rust-analyzer, CSS and HTML servers
  advertise pull too). Pull-only servers (`pull_diagnostics`, TypeScript 7's `tsc --lsp`)
  are asked after open, change and save, and on `workspace/diagnostic/refresh`; a pull
  superseded in the writer's queue is dropped.
- **Hints are not shown**: servers use them for faded code and to repeat related locations.
- **Servers**: rust-analyzer, clangd, `tsc --lsp --stdio` (native, instead of
  typescript-language-server), vscode-html/css-language-server.

## Core additions

- `Registry::status_providers` / `StatusSegment`: clickable groups in the status pill.
- `Icon::{Error, Warning, Info}`, `Face::{Error, Warning, Info}` (underlines).
- `Candidate::{group, detail}` may be owned strings.
- `Editor::buffers_mut`, `Editor::cancel_minibuffer`.

## Open

- Servers are found on `PATH`; an app started from Finder does not see Homebrew's.
- No server restart after a crash, no progress (`$/progress`) in the status pill.
- Server configuration (`workspace/configuration`) is answered with defaults.
