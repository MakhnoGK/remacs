# 0012 — Dired: marks and deletion

Status: accepted, 2026-10-04 (dired, step 3)

## Decision

- **Marks live beside the text**, in `Listing::marks` by entry name, not as characters in
  the buffer as in Emacs: the buffer's lines stay the names (step 5 edits them), and
  marks survive sorting and re-reads; marks of files that disappear go with them. `..`
  is never marked. Core `Row::mark` (`Mark::Marked`/`Flagged`) draws a check or a `D`
  in a slot before the icon, tints the row and strikes flagged names through.
- **A y-or-n question can be a dialog**: `Editor::ask_dialog` takes a `Dialog` (title,
  explanation, items with their marks, one option, button labels, destructive) drawn
  from the option's state, so text that depends on it ("Move to Trash" / "Delete")
  follows it. `y`/`n`, the option's key and clicks on the buttons or the option answer
  it; `C-g` cancels as before.
- **Deletion runs in the background** and reports when done; the listing is read again
  then (FSEvents would do it too). It moves to the Trash through `NSFileManager` (the
  `trash` crate): Finder's AppleScript way asks for automation permission and plays a
  sound. Switched off, it deletes for good, directories with everything in them; a link
  is removed, not what it points to.
- **Status**: `StatusItem` gains a `Tone`, so `2 marked` / `2 flagged` show in the accent
  and danger colors before the item count.
- `+` creates a folder (`create_dir_all`, so `a/b` works) and puts point on it once it
  is listed.

## Keys

`m` mark, `u` unmark, `DEL` unmark backward, `U` unmark all, `t` toggle marks, `d` flag,
`~` flag backups, `#` flag auto-saves, `x` delete flagged, `D` delete marked (or the
file at point), `+` new folder. In the dialog `t` switches "Move to Trash".
