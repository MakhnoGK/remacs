# 0039 — Creating a file from dired in a row of its own

Status: accepted, 2026-10-05

## Decision

- **`c` (`dired-create-empty-file`) opens a row under point** with a field for the name,
  drawn like a name wdired edits. `RET` (or `C-c C-c`) creates the file; `C-g`, `ESC`
  (or `C-c C-k`) and an empty name remove the row. Emacs leaves its command unbound and
  `+` already makes folders.
- **The row is an empty entry in the listing** (`Listing::new_file` holds its line), so
  lines and entries stay one to one. Meanwhile the listing is frozen like wdired's: not
  read again, not sorted, no preview. Not wdired itself: that mode undoes any change to
  the number of lines.
- **The name stays on its line**: a command that changes the line count is undone, and
  point is kept in the row.
- **A name already listed is refused**, compared case-insensitively as macOS volumes are
  by default; the echo says so and the row stays to correct it. `File::create_new` refuses
  what appeared since the read. Names are checked like wdired's (`invalid_name`).
- **After creating, point is on the new file, which is not opened**, so it can be
  renamed or opened next.
