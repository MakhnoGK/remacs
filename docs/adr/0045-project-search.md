# 0045 — Searching the project's text

Status: accepted, 2026-10-06 (issue #7)

## Decision

- **A command of its own, `project-search`** (`M-s g`, `s-F`), like Emacs
  `consult-ripgrep`, rather than one prompt for files, symbols and text (user's choice):
  `find-file` and `xref-find-apropos` already search file names and symbols by flex
  match. It lives in the search plugin, beside `C-s`.
- **Words, literal, smart case** (user's choice): the input's space-separated words must
  all be on a line, in any order; ASCII case is ignored unless the input has an uppercase
  letter, as in the other prompts. No regex, no fuzzy match over lines: exact results,
  and fast on gigabytes.
- **The files are find-file's index** (files plugin `project_files`, leaving out what
  `files.toml` says, ADR 0046, at most 100 000 files), so nothing walks the tree again. Until its first
  scan finishes the prompt says "Searching…"; once the files change, the search runs
  again. Buffers with unsaved changes are searched as they are, first.
- **Background threads, one search at a time**: up to 8 threads take files in turn; a
  new input cancels the running search. Files under 32 MiB are read into a buffer each
  thread reuses, larger ones mapped; a NUL in the first 8 KiB marks a file binary and
  skips it. The rarest (longest) word is looked for with `memchr::memmem`, the others
  checked on its lines; case is ignored by lowering 1 MiB chunks, ending at a line end,
  into a reused buffer. A full pass over 1.9 GB (42 000 files and one of 850 MB) takes
  about 0.6 s, ripgrep 0.75–1.1 s on the same tree.
- **The list**: lines grouped by file, in the index's order (shallow files first), the
  words lit, the line number beside each; long lines show the part around the first
  match. The prompt is told of new lines at most every 40 ms. RET visits the file with
  point on the line's first match.
- **`search.toml`**: `project-max-matches` (10 000) stops the search there and says
  "First N lines".
- **The window follows the selection** (user's choice over a pane in the prompt), like
  consult's preview: the selected line's file is shown behind the prompt, point on the
  match, the words lit on the lines drawn and the selected line's apart (up to 4 KiB of
  a line). `C-g` puts back the buffer, point and view; files that had no buffer before
  are closed again, except the one chosen with RET.

## Later

Replacing across the project (#9).
