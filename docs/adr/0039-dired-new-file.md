# 0039 — Creating a file from dired through the prompt, like a folder

Status: accepted, 2026-10-05; revised 2026-10-20 (#99): the in-place row is gone.

## Decision

- **`c` (`dired-create-empty-file`) reads the name in the minibuffer prompt**, the same
  one `+` (`dired-create-directory`) uses: label "New file", the listed directory on its
  info line, ↵ Create / C-g Cancel. One way of creating things. Emacs leaves its command
  unbound.
- **A name with `/` creates the missing folders**, as `+` does; an existing name is an
  error (`symlink_metadata`, which a case-insensitive volume answers for any letter case),
  and `File::create_new` refuses what appeared meanwhile.
- **After creating, the listing is read again with point on the new file**, or on the
  first folder of a deeper path; the file is not opened, so it can be renamed or opened
  next.
- **File → New File… and New Folder…** run the two commands, enabled in a listing.

The first version opened an editable row under point, with its own finish and cancel
commands and keys and a frozen listing meanwhile; two ways of naming a new entry was one
too many.
