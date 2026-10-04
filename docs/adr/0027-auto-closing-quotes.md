# 0027 — Auto-closing quotes and the syntax context

Status: accepted, 2026-10-04 (issue #1 editing experience, step 4)

## Decision

- **Quotes are pairs whose opener is their closer**, in `Mode::pairs` (ADR 0026): `"`
  everywhere; `'` in C, C++, CSS, HTML and JS/TS (not Rust, where it starts lifetimes);
  `` ` `` in JS/TS only.
- **A quote pairs only in code**, not after a word character (`don't`) and only before
  what an opener pairs before; never before another quote.
- **A quote steps over the same quote** when point is in a string, or right after the same
  quote, so an empty pair just typed is closed even before it is parsed.
- With a region, a quote wraps it like a bracket; `DEL` deletes an empty quote pair.
- **Syntax context in core**: `Editor::syntax_at(offset)` says `Code`, `String` or
  `Comment`, asked of `Registry::syntax_probes`; code when no plugin knows the buffer. The
  `syntax` plugin answers from the buffer's tree: the node holding the character before
  the offset and its ancestors. Kinds containing `string`, `char_literal` or
  `quoted_attribute_value` are strings when they strictly contain the offset; kinds
  containing `comment` are comments, including at the end of a line comment but not after
  `*/` or `-->`; a template substitution or interpolation inside a string is code again.
- The tree is read as of the last command; between a keystroke and its reparse it may be
  one edit behind, which the empty-pair rule above covers.

## Open

- Brackets still pair and count inside strings and comments.
