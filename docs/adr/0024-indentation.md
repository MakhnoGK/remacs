# 0024 — Indentation on new lines

Status: accepted, 2026-10-04 (issue #1 editing experience, step 1)

## Decision

- **`RET` indents the new line** from the line it breaks: a level deeper when a bracket
  `{ ( [` opened on that line before point is still open; at the level of the line
  holding its opener when point stands before a closer `} ) ]`; otherwise at the same
  level. Between an empty pair (`{|}`) the closer moves to a line of its own and point
  stays on the deeper line between.
- **No trailing blanks**: blanks around point go before the break, so a line holding only
  blanks is emptied and the new line keeps its indentation.
- **A closer typed on a blank line** first indents that line to its opener's level
  (electric dedent), in the same undo step as the character.
- **The indent unit comes from the file**: tabs if more lines start with a tab than with
  spaces, else the most common step between consecutive lines (2–8; a step of one is a
  block-comment ` * `). It is detected from the first 64 KiB, kept per buffer once found,
  and 4 spaces until the text shows one. `TAB` (`tab-to-tab-stop`) uses it too.
- **Brackets, not grammars**: the rules count brackets, plain text included, ignoring
  strings and comments. Every scan is bounded to 64 KiB, so gigabyte files stay instant.

## Open

- Brackets inside strings and comments are counted; the syntax context from step 4 can
  refine that.
- HTML tags do not indent.
