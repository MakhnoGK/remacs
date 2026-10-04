# 0026 — Auto-closing brackets

Status: accepted, 2026-10-04 (issue #1 editing experience, step 3)

## Decision

- **Pairs belong to the major mode**: `Mode::pairs`, filled by `languages`. `() [] {}`
  everywhere; `<>` only in HTML, JSX and TSX, since elsewhere `<` is also less-than (help
  with generics is left to snippets). Fundamental mode — plain text and every prompt —
  pairs nothing.
- **Typing an opener** inserts its closer too, point between, when the next character is
  a blank, the end of the line, a closer or one of `; : . , = >`. Before a word it stays
  single, so wrapping an existing expression by hand leaves no stray closer.
- **With a region**, an opener encloses it and keeps it marked, so it can be wrapped again.
- **Typing a closer** steps over the same closer after point when the line already closes
  as many as it opens; otherwise it is typed, so a missing closer can still be added.
  Balance, not memory of what was auto-inserted: nothing to keep in sync with edits.
- **`DEL` between an empty pair** deletes both.
- Undo: the pair is part of the typing it came with (ADR 0004 amalgamation).

## Open

- Brackets in strings and comments count toward the balance; step 4 brings the syntax
  context.
