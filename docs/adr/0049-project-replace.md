# 0049 — Replacing across the project

Status: accepted, 2026-10-07 (issue #9)

## Decision

- **`query-replace` across files** (user's choice over replacing from `project-search` or
  editing its results): `project-query-replace` (`M-s %`, `s-H`) in the search plugin.
- **The text to replace is typed in `project-search`'s prompt**, listing the lines that
  have it by file with the window following the selection (ADR 0045), but taken whole,
  spaces included, literal with smart case like `M-%` (`Words::phrase`). RET takes the
  input, whichever line is selected; then a prompt for the replacement, sharing
  `query-replace`'s history.
- **Then one match at a time**, the file shown behind a read-only prompt that names it,
  the match lit apart: `y`/`SPC` replace, `n`/`DEL` skip, `!` the rest of the file, `A`
  everything left in every file without asking, `q`/`RET`/`C-g` stop, keeping what was
  replaced.
- **Files are found in the background**: once find-file's index is complete, a search
  of the same files (unsaved buffers as they are) notes only which ones have the text,
  one line each, with no limit on files. They are shown in the index's order as they
  are found; at the end of the known ones the prompt says "Searching…" and goes on as
  more turn up. Each file is searched again in its buffer when shown.
- **Every file changed is saved** (user's choice, as with renames, ADR 0044), with what
  it had unsaved. Its replacements undo at once. Files opened only to be shown are
  closed when done with, and the window goes back to where it was.
