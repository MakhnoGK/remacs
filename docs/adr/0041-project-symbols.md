# 0041 — Finding symbols of the project by name

Status: accepted, 2026-10-05

## Decision

- **Part of xref, like Emacs `xref-find-apropos`** (`C-M-.`): the user's choice over a
  plugin of its own; going to a symbol is remembered, so `M-,` comes back. Symbols of the
  open file (`textDocument/documentSymbol`) come later, if at all (user: secondary).
- **xref asks, backends answer**, as for definitions: as the name is typed, xref queues a
  `SymbolRequest` (the query and the file searched from) and takes `deliver_symbols` for
  the newest one only; an answer to it may come more than once and replaces the last.
- **The prompt is the other searches'**: each row is the name with its matches lit, what
  it is declared in, its kind as a tag and `path:line` from the project root (the Git work
  tree). The backends' order is kept. While a newer answer is awaited, the last one is
  narrowed to the input with the flex match, so the list keeps up with typing; "Searching…"
  until there is anything.
- **lsp asks every server of the project** (`workspace/symbol`; those of the file searched
  from, started if needed, else those running for its directory) and delivers once all
  have answered, in the servers' order. A server that answered while still loading the
  project is asked again once it is done, and the list updates by itself. Servers without
  `workspaceSymbolProvider` answer nothing at once. Symbols are placed in the files they
  name, each file read once; ones given by file only (`WorkspaceLocation`) are left out.
- The client advertises every symbol kind (1–26); kinds map onto the editor's few, so
  structs, enums and classes all read as `type`.
- **The window follows the selection** (2026-10-07, user), as in `project-search`: the
  selected symbol's file is shown, point on it. `C-g` puts back the buffer, point and
  view and closes the files opened only to be shown; RET goes back first, so `M-,`
  returns to where the search started. The bookkeeping is the files plugin's
  `FilePreview`, shared with the project searches.
