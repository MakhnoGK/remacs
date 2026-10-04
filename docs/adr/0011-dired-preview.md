# 0011 — Dired: preview pane

Status: accepted, 2026-10-04 (dired, step 2)

## Decision

- **A table may carry a `Preview`** of its selected row: icon, title, kind, a body and
  label/value details. The body is code (the head of a file), items (a directory's
  names) or a message (binary, empty, an error; empty while it is read). The UI draws it
  beside the rows when the window is wide enough, else leaves it out.
- **Read in the background**: each listing owns a `PreviewReader` thread. A post-command
  hook asks it for the entry at point whenever the entry, its size or its modified time
  changes, and takes in what was read. A file's head (32 KiB, at most 120 lines, tabs
  expanded, lines cut at 240 columns; a NUL byte means binary) arrives first, its line
  count after, read a 1 MiB chunk at a time and abandoned as soon as another entry is
  asked for, so gigabyte files cost nothing once point moves on.
- **Highlighted on the UI thread** through `Editor::highlight_code`, sharing the snippet
  face cache with documentation popups; the head is small enough to parse whole.
- **Details**: path, size (`· N lines` once counted, items for directories), modified
  (`Today, 10:36`, `Yesterday, …`, else the date with time), permissions.
- **`Editor::notifier()` forwards to whichever notifier is set when it is called**, so
  threads started before the UI exists (`remacs <dir>`) still wake it.

## Later

The Git line of the details comes with the Git milestone.
