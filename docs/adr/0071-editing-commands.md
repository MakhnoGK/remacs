# 0071 — Editing: more cursors, iedit, rectangles, structural edits, line commands, wgrep

Status: accepted, 2026-10-18

The editing half of issue #71: Emacs editing commands remacs lacked.

## Decision

- **Cursors live in `Buffer`** (`Cursor`: point, mark, whether it is active, and a kill
  ring of its own), ordered by point and moved by edits like the mark. A command marked
  `Command::each_cursor` runs at point, then again at each cursor, with that cursor's
  point, mark and kill ring swapped in, all in one undo group; cursors that run into each
  other merge, also before such a command, as one run once may have moved them together.
  Each edit moves every cursor after it, so a key costs the square of their number: 7.6 ms
  at 1000, 0.8 s at 10000 (`benches/cursors.rs`); `max-cursors` in `editing.toml` (1000)
  refuses more. The marked commands are an allow-list (motion, typing, deleting, killing,
  yanking, transposing, case, joining, duplicating, sexp motion), the reverse of
  multiple-cursors' run-once list: a command that reads keys or text after it, keeps
  state between runs, or moves far (paging, buffer ends) runs once, and a command a
  plugin adds runs once unless the plugin marks it. A new cursor's kill ring is a copy of
  the kill ring as it is; entries are `Rc<str>`, so a copy is cheap. Errors at cursors
  other than point are not shown. `C-g` takes the regions away, then the cursors.
- **iedit is the same cursors**, one at each whole-symbol occurrence of the symbol at
  point, or each occurrence of the region, as far into it as point is; `C-;` again
  removes them. Edits then happen at every occurrence as they do with multiple cursors,
  rather than through linked occurrences.
- **Rectangles** are the character columns between point and mark on the lines between
  them; tabs count as one column. `rectangle-mark-mode` is a flag on the active region
  (`Buffer::is_rectangle`), dropped whenever the region is activated anew; while it is
  set, kill, copy and delete act on the rectangle. A killed rectangle is also pushed to
  the kill ring as its lines, so the clipboard gets it, and `yank` inserts it as a
  rectangle while it is the newest kill, as Emacs' yank handler does.
- **Structural edits** (`transpose-sexps`, `kill-sexp`, `mark-sexp`, slurp, barf,
  splice, raise) are in the `syntax` plugin over the tree, as sexp motion is (ADR 0070);
  slurp and barf skip `,` and `;`. They refuse until the tree is reparsed after the last
  edit, as a tree only shifted to follow an edit mangles a second one.
- **`zap-to-char` reads its character through a key sink**, like avy, not a prompt.
- **Line commands** (`sort-lines`, `keep-lines`, `flush-lines`,
  `delete-duplicate-lines`, `align-regexp`) take the region's whole lines, or the
  buffer's; regexps are the `regex` crate's syntax, not Emacs'. `align-regexp-repeat` is
  Emacs' `C-u align-regexp` with repeat, as remacs has no prefix argument.
- **wgrep: `C-x C-q` in the project-search prompt** closes it and lists its lines as
  `file:line:text` in `*search results*`. `C-c C-c` writes the changed texts: to the
  buffer visiting the file, saved unless it had unsaved changes, else to the file; a line
  whose text changed since the search is left and counted. Adding or removing lines, or
  editing a `file:line:` prefix, is refused at writing rather than prevented.

## Later

- Display columns for rectangles and `align-regexp`, for text with tabs.
- `zap-to-char` and prompts at every cursor.
- Read-only `file:line:` prefixes in `*search results*`.
