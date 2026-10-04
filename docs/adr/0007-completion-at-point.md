# 0007 — Completion at point

Status: accepted, 2026-10-04 (M4, step 3)

## Decision

- **Two plugins, like corfu and eglot.** `completion` owns the popup (session, matching,
  keys, insertion) and knows nothing of LSP; `lsp` is one backend. The completion plugin
  queues `Request`s (`Complete`, `Resolve`) that the backend takes in its post-command
  hook and answers with `deliver`/`resolved`, the same direction as `diagnostics::publish`.
  `completion` is registered before `lsp`, so a request goes out in the same pass.
- **Automatic, like VS Code.** Typing a word character or one of the server's trigger
  characters asks for a list; the session lasts while the word is typed or erased, any
  other command ends it. `C-M-i`/`M-TAB` asks explicitly.
- **One request per word.** While a list for the same word start is on its way, typing
  does not ask again; the answer is narrowed to the longer word. Incomplete lists are asked
  for again as the word grows. Answers to anything but the newest request are dropped, and
  the writer drops a completion superseded in its queue.
- **Matching**: flex (characters in order, smart case) against `filterText`; prefix matches
  first, then the server's `sortText` order. Highlights are matched on the label.
- **Insertion** replaces the word from the item's edit start to point; `additionalTextEdits`
  before the word are applied (imports). One command, one undo step.
- **Snippets**: advertised, because the CSS and HTML servers complete only for clients
  with snippets. Expanded to plain text, placeholders keep their text, point goes to the
  first tab stop; there is no tabbing through fields. rust-analyzer (`callable.snippets =
  add_parentheses`) and clangd (`--function-arg-placeholders=0`) complete calls to `f()`
  with point inside, not to argument names.
- **Documentation**: the item's `detail` is the signature, Markdown is reduced to paragraphs
  and code blocks. Items without either are resolved (`completionItem/resolve`) once, when
  selected; TypeScript sends documentation only this way.
- **Popup hides when point moves without a command** (mouse); typing on in the word shows
  it again, any other command ends the session.

## Core additions

- `Popup`, `Documentation`/`DocBlock`, `Registry::popup_providers`: the UI draws whatever a
  plugin describes, anchored to a buffer offset, below the line or above when it does not
  fit, the documentation beside it.
- `Registry::minor_keymaps` (`MinorKeymap`): keymaps active while a predicate holds.
- `Editor::command_count`, `Editor::find_plugin_state`, `SymbolKind`, `Candidate::kind`,
  `flex_match`, `Selection` (shared with the minibuffer).

## Open

- Items resolved after insertion lose what resolving adds (TypeScript auto-imports when
  the item was inserted before its documentation arrived).
- No signature help, no commit characters, no `command` after insertion.
- Rows show the server's `labelDetails.description` or `detail`, which rust-analyzer fills
  with the whole signature, not just the type.
