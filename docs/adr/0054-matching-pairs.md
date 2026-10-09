# 0054 — Matching brackets and tags

Status: accepted, 2026-10-09

## Decision

- The bracket after point, else the one before, is highlighted with its partner, like
  Emacs `show-paren-mode`: `()`, `[]`, `{}` and `<>`. A tag point is on (its name or its
  own `<`, `>`, `</`, `/>`) highlights both tags' names: HTML, XML and JSX (a fragment's
  `<>`/`</>` whole). Attributes do not count as the tag.
- Pairs come from the syntax tree, not the text: brackets in strings and comments, and
  `<` as an operator, have no partner. While the tree lags behind an edit nothing is
  shown, rather than a wrong pair.
- One face, `MatchingPair`: a light accent background and a light accent foreground
  (user's choice, to be seen better than a background alone).
- `syntax.toml` `matching-pairs = false` turns it off.

## Later

- Mode-specific brackets (`|…|` closures), and a mismatched bracket shown as an error.
