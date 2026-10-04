# 0014 — Dired: editing names in place

Status: accepted, 2026-10-04 (dired, step 5)

## Decision

- **The listing's lines are the names** (ADR 0010), so editing them is editing the buffer,
  like Emacs wdired: `w` or `C-x C-q` makes it writable and every editing command works.
  The dired keys give way to typing; `Listing::editing` holds the state. While editing,
  reads and sorting are held back, since each line stands for the entry it was drawn
  from; cancelling reads the directory again.
- **One name per line, `..` untouched**: a post-command hook undoes a command that adds or
  joins lines or edits `..` and says why, so `C-k`, `C-d`, yanks and the rest stay safe
  without a list of forbidden commands.
- **Drawn as fields**: core `Row::field` turns the first cell into a text field with
  point's cursor and region, tinted once edited and followed by `was <name>`; `Notice`
  replaces the sort controls; `Changes` (renames, what they update elsewhere, toggles)
  replaces the preview. `Tone::Warning` shows `Editing · 2 changes`.
- **Applying** (`C-c C-c`) refuses empty names, `/`, `.`, `..` and two entries sharing a
  name (case-insensitively, as APFS sees it); moving elsewhere is `R`'s job. Renames run in
  the background in two steps through temporary names, so swaps, chains and case-only
  changes work, and with `renamex_np(RENAME_EXCL)`, so nothing already there is replaced.
  Buffers of renamed files follow them.
- **References** (`Also updates`): dired asks backends (`take_rename_requests` /
  `deliver_rename_edits`) about the renames after every change; `lsp` sends
  `workspace/willRenameFiles` to the servers of the renamed files (started if needed) and
  of directories in projects already served. rust-analyzer answers `null` while loading
  the project, so the client opts into `experimental/serverStatus` and asks again once it
  is quiescent; meanwhile the pane says "Checking other files…" and applying waits. The
  edits are made before renaming, and files without unsaved changes are saved, so disk
  stays consistent. "Update references" switches them off.
- **`M-%` replaces in every name** (two prompts, no queries); one undo takes it back.
- `FileEdits`, `Buffer::apply_edits` and `Editor::edit_file` moved to core, shared with
  code actions.

## Keys

`w`, `C-x C-q` edit names; then `C-c C-c` / `C-x C-s` apply, `C-c C-k` / `C-c ESC`
cancel, `TAB` / `RET` next name, `S-TAB` previous, `M-%` replace in names.

## Later

"Rename with git mv" and the Git column come with the Git milestone.
