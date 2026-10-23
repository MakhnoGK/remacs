# 0083 — Queries of one's own, and highlighting without a grammar

Status: accepted, 2026-10-23 (issue #102, steps 7–8)

## Decision

- **Queries of one's own** go in `~/.config/remacs/queries/<mode>/highlights.scm`, the
  directory named after the mode, ignoring case. A first line `; extends` appends the
  file to remacs's query, so its patterns win (ADR 0081: the last pattern wins); any
  other file takes remacs's place. They are read at start and on
  `reload-syntax-queries`; one that does not compile is named in an error and remacs's
  query stays. The parser thread compiles highlighters anew when the set changes, and
  each buffer is parsed again with them.
- Only highlight queries for now: injections and the code popups' highlighting keep
  remacs's queries.
- **Without a grammar** — a mode that has none, or a file over the 16 MiB parsed at most —
  each visible line is read alone, as Emacs's font-lock reads keywords: the mode's comment
  syntax (a block comment only when it closes on the line), its quotes as strings, and
  numbers. TODO keywords (ADR 0082) show in those comments too.
