# 0092 — Implementations, type definitions, call and type hierarchies

Status: accepted, 2026-11-04 (issue #203)

## Decision

- **All in `xref`, fed by `lsp`**, as definitions are (ADR 0008): `xref` owns the
  commands, keys and views and knows no LSP; `lsp` takes its requests and delivers. Every
  server that offers them gets them, not only rust-analyzer.
- **Implementations and type definitions are lookups** like definitions:
  `xref-find-implementations` (`M-g M-i`) and `xref-find-type-definitions` (`M-g M-t`).
  One place is gone to, several are listed in the minibuffer, `M-,` comes back.
- **Offered only when a server can.** What a server offers is read from its
  `initialize` answer into one `Offered`, shared by the client's writer and the editor:
  a request it cannot answer is never sent, and the Go menu's items for these four
  commands are greyed out where the server of the code at point lacks them. A server
  that lacks one says so: "vscode-css-language-server cannot find implementations",
  not "No implementations found". Until a server has answered `initialize`, everything
  is taken as offered, since the writer holds requests until then.
- **Hierarchies are a pane**, like the debugger's (ADR 0051), not a buffer: the tree
  stays beside the text it leads to, and rows open with `▸`/`▾` and indent for free
  (`PaneRow::depth`, `open`). `xref-call-hierarchy` (`M-g M-c`) starts at the callers
  of the function at point, `xref-type-hierarchy` (`M-g M-y`) at the supertypes of the
  type at point. The pane takes the keys: `TAB` opens a row, `RET` goes to it and gives
  the keys back, `d` flips to callees or subtypes from the same item, `q` closes, `C-g`
  leaves. Its height is `hierarchy-pane-height` in `xref.toml`.
- **Rows open as they are asked for**: the first row opens at once; each other row asks
  its server for its children the first time it opens, and keeps them. A flip drops the
  tree under the roots and asks again.
- **Items are asked about by the server that gave them**, not the window buffer's:
  `lsp` keeps each item as the server sent it, with its client, under the key it hands
  `xref`, so rows open whatever file the window shows. A new tree's roots, once they
  come, replace the items kept; a tree that fails to start leaves the one shown working.
  The client is held weakly, so a server restarted or stopped exits rather than living
  on for the tree; its rows then say to open the hierarchy again, and so does a request
  whose server stopped before answering, rather than asking for good.
- **Failures are said, never shown as empty.** A server's error, or an empty answer
  while it is still loading the project, is said in the echo area and closes the row,
  which asks again when opened; only a settled server's empty answer means "none".
- **A caller's or callee's row goes to its call**, in the caller (`fromRanges`): for a
  caller, where it calls the row above; for a callee, where the row above calls it. The
  place shows the first call's line and, when there are more, how many (`main.rs:4 ×2`).
  Types and the tree's roots go to their names.

## Later

- Only a row's first call is gone to; each call as its own row once callers that call
  many times need them apart.
- Type hierarchies are checked with clangd only: rust-analyzer offers none.
