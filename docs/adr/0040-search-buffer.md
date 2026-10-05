# 0040 — Searching the buffer in a prompt of matching lines

Status: accepted, 2026-10-05

## Decision

- **Like the other searches, not a bar or isearch** (user's choice): `C-s`
  (`search-buffer`, plugin `search`) opens the prompt that finding files and definitions
  use, listing the lines that contain the input, like Emacs `consult-line`. Each shows
  its number, the matches lit; `C-s`/`C-r` select the next or previous, `RET` goes to the
  line's first match, centered if off screen, and leaves the mark where the search
  started.
- **Literal text, smart case**: ASCII case is ignored unless the input has an uppercase
  letter, as in the prompts' filters. No regular expressions for now (user).
- **The first match after point is selected**, through `Completions::selected`, a new
  field that picks the selection for an input.
- **Gigabyte files**: the text is searched a chunk at a time with `memchr::memmem`,
  keeping enough of the previous chunk for a match that crosses the boundary. At most
  10,000 lines are listed; past that the counter says so, so that a common string still
  answers each key at once.
- **Preview while the prompt is open**, like isearch: the matches on screen are lit
  (`Face::SearchMatch`, the selected one `Face::SearchCurrent`) and point follows the
  selection, centered when it leaves the screen. Quitting puts point and the view back;
  for that, quitting with `C-g` now runs the post-command hooks too. What is lit lives on
  the buffer (`Lit`), since decorators see only the buffer; the decorator searches just
  the lines drawn.
- **Replacing, like Emacs `query-replace`** (`M-%`): a prompt for the text, its matches
  lit and counted as it is typed, then one for what replaces it, then a read-only prompt
  at each match from point to the end: `y`/`SPC` replace, `n`/`DEL` skip, `!` replace the
  rest as one edit, `q`/`RET`/`C-g` stop, keeping what was replaced. Every replacement
  undoes at once: the editor puts undo boundaries in the current buffer, which is the
  prompt's input while it is open, so only the boundaries at the start and the end fall
  in the replaced buffer.

## Not covered

- Case is not carried over: with lowercase text to replace, `Total` becomes the
  replacement as typed, not capitalized like Emacs `case-replace` does.
