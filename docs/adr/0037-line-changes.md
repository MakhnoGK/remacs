# 0037 — Changed lines in the gutter

Status: accepted, 2026-10-05 (design board 28)

## Decision

- **Against `HEAD`**, like Emacs diff-hl: every change not yet committed shows, staged or
  not. Unsaved edits show too: the buffer's text is compared, not the file on disk.
- **Board 28's marks**: a 3 px bar between the line numbers and the text, green beside
  added lines and blue beside modified ones; a red 9×3 tick between the lines where some
  were removed. In a hunk, as many lines as were removed count as modified, the rest as
  added, or as a removal after them when fewer took their place.
- **The base is the file as checkout writes it**: `git cat-file --filters <commit>:<path>`,
  so line endings and smudge filters compare equal. Read once per commit and path; a
  renamed file is compared with its name before the rename. Untracked and newly added
  files are all added lines; a file Git ignores has no marks.
- **One background thread** compares for every buffer with `imara-diff` (histogram, git's
  line sliders), keeping only the newest text per buffer. No idle delay: a request queued
  while a comparison runs replaces older ones. Only the buffer in the window is compared;
  buffers over 16 MiB are not.

## Core additions

- `ChangedLines`/`LineChange` and `VersionControl::line_changes`; the UI asks
  `Editor::line_changes` for the drawn lines and knows nothing of Git.
