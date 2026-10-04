# 0010 — Dired: directory listing

Status: accepted, 2026-10-04 (dired, step 1)

## Decision

- **A listing is a read-only text buffer** whose lines are the entries' names, `..`
  first, in display order, as in Emacs. Motion, `C-v`, `M-<` work unchanged; point's line
  is the selected row. Names as text leave room for editing them in place (step 5).
- **Drawn as a table by the UI.** Core gains `Table` (crumbs, sort buttons, columns,
  rows, hints, a double-click command) and `Registry::tables`; a provider is asked only
  for the rows on screen. The UI knows nothing of directories. Clickable controls carry
  an `Action` closure, run through `Editor::run_action` like a command.
- **Background read, FSEvents refresh**: each listing owns a `DirectoryReader` thread
  that reads the directory (stat, item counts of subdirectories, formatted columns), then
  watches it non-recursively with `notify` and reads again 50 ms after changes settle.
  A post-command hook takes the newest listing and keeps point on its entry.
- **Navigation replaces the listing** (`RET` on a directory, `^`, crumbs), like Emacs
  `dired-kill-when-opening-new-dired-buffer`; an open listing of the target is reused.
  `^` and `C-x C-j` put point on the directory or file they came from.
- **Order**: `..`, directories, files; Name (case-folded), Modified (newest first) or
  Size (biggest first, items for directories). Hidden, backup and auto-save files dim.
- **Read-only buffers**: `Buffer::read_only`, checked by the editing commands
  (`ensure_writable`, Emacs `barf-if-buffer-read-only`). `Buffer::default_directory`
  replaces the file's parent for relative names, so `C-x C-f` from a listing starts there.
- `remacs <dir>` opens a listing. Sizes are decimal (Finder), dates `Oct 4, 10:41`, or
  with the year when not this year (`ls -l`).

## Keys

`C-x d` dired (directory prompt, `./` first), `C-x C-j` dired-jump; in a listing
`RET`/`f`/`e` open, `^` up, `n`/`SPC`/`p` move, `s` cycles the sort, `g` re-reads.

## Later

Preview pane (step 2, ADR 0011), marks and deletion (3), copy/move (4), editing names (5); the Git
column and preview line come with the Git milestone.
