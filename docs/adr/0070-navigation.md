# 0070 — Navigation: mark ring, back and forward, last change, sexps, imenu, avy

Status: accepted, 2026-10-18

The navigation half of issue #71: Emacs motion commands remacs lacked.

## Decision

- **The mark ring lives in `Buffer`** (`push_mark`, `pop_mark`), moved by edits like the
  mark; 16 entries, like `mark-ring-max`. `set-mark-command`, `beginning-of-buffer`,
  `end-of-buffer` (unless selecting), `mark-whole-buffer`, `search-buffer`, `imenu` and
  avy push it, as in Emacs. `C-u C-SPC` is a key sequence of its own, as remacs has no
  prefix argument; `C-SPC` right after it pops on (`set-mark-command-repeat-pop`).
- **Back and forward (`s-[`, `s-]`, `C-x C-SPC`) are a new `navigation` plugin**, not
  `push-mark`'s global ring. A jump is seen after each command with no prompt open: a
  move to another buffer, or across `jump-lines` (10) lines. A search or a symbol list
  counts once, from where it started to where it ended, whatever its preview showed.
  Within a buffer, scrolling, by a command or the mouse (`Editor::scrolled`), is not a
  jump, nor, between commands, a move with the text changed, as point follows a console's
  output; a click or an avy label leaves the text as it was, so it is one. Going back
  first sees a move not yet seen. No jumping command has to report itself. avy searches
  only the columns on screen, so a line of megabytes costs no more than a short one.
  Places are plain
  offsets with the buffer's file, so a killed file opens again; edits before a place
  leave it off by their length.
- **`goto-last-change` reads the undo history** (`Text::change_offsets`): each group's
  edit is the difference of its two states, moved through the edits since, so places stay
  right after later edits above them. One place per line, as typing makes many groups.
- **Sexp motion is in the `syntax` plugin, over the tree**: the expressions point is
  between are the children of the innermost node it is inside of; a list is a node whose
  first and last children are a bracket pair (`()`, `[]`, `{}`), and motion stays between
  the brackets of the list it is in. No fallback for buffers without a tree.
- **`imenu` asks servers for `textDocument/documentSymbol`** through xref, like
  `xref-find-apropos`; nested symbols are listed after what they are in, which is their
  annotation. The window shows the symbol selected.
- **avy draws `TextLabel`s**, a new provider kind, over the window buffer's text, in the
  accent color. It reads keys through a key sink. Labels are all as long as the most
  needs, the first keys on the places nearest point.

## Later

- Labels shorter for nearer places, as avy's tree gives them.
- `imenu` from the tree for files no server serves.
