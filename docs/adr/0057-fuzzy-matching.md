# 0057 — Prompts match the way fzf does

Status: accepted, 2026-10-10

## Decision

- **Every prompt matches fzf-style** (user: like fzf-lua in Neovim), through
  `remacs_core::FuzzyQuery` over `nucleo-matcher`, helix's port of fzf's algorithm. A
  term matches its characters in order; space-separated terms match in any order;
  `'term` matches as written, `^term` at the start, `term$` at the end, `!term` not at
  all. A term ignores case unless it has an uppercase letter. This replaces matching
  space-separated substrings.
- **find-file matches the whole relative path**, so `plufifi` finds
  `plugins/files/src/find_file.rs` without typing a `/`. The old split (only the file
  name until the input had a `/`) is gone, and rows show the path with its matched
  characters highlighted.
- **Ranking in find-file**: the higher score first (fzf's bonuses for word starts after
  `/`, `_`, `-`, and for runs), then paths whose file name matches the query too, as
  fzf's `--scheme=path` ranks, then shorter paths, then the index's order (shallow
  files first). Adding the file name's score instead made scattered matches inside long
  names beat clean path matches.
- **Speed**: lists over 8192 items are scored on all cores (`filter_map_parallel`), and
  only the best 500 are sorted. On 100 000 paths a keystroke takes 1–17 ms, compared with
  15–80 ms on one thread.
- Prompts that keep their own order (buffers, branches, the kill ring) still do;
  prompts that rank (M-x, find-file) rank by score.
- Unchanged: in-buffer completion keeps Emacs `flex` (`flex_match`), xref-find-apropos
  keeps the language server's ranking, and searching text (C-s, project-search) stays
  literal.
