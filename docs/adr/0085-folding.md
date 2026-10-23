# 0085 — Folding blocks of code

Status: accepted, 2026-10-23 (issue #102, step 5b)

## Decision

- **Folds are a buffer's, like Emacs hideshow's overlays**: each hides whole lines after
  its header line, kept as byte ranges with the text they were placed in. They follow
  every insert and delete at once; edits undo or a revert makes are followed after the
  command, by one diff of the text, so folds between two such edits go. A fold an edit
  touches goes; so does one point ends up in after a command (a search, `goto-line`, a
  definition), as hideshow shows a block isearch enters. While a prompt is open, as a
  search previews its matches, those folds come back if it is quit, unless the text
  changed since; answered, they stay open.
- **Windows count rows, not lines**: `ShownLines` maps a buffer's lines to the rows they
  are shown on, folded lines on their header's row. `Viewport`'s height, margins and
  scrolling count rows, its `first_line` stays a line; the text, the gutter's change
  bars, the mouse, smooth scrolling, popups, `C-n`/`C-p`, `M-r`, `C-l` and avy go by
  rows. A header shows `⋯ N lines` after its text, as a line note. Folding or unfolding
  moves the text to its new rows at once, not by smooth scrolling.
- **Blocks come from the syntax tree**: the indent query's `@indent.begin` nodes
  (ADR 0084) over more than one line, or, for a mode without one, its lists between
  brackets. Of those starting on one line the outermost is the block. A block's closing
  line stays shown when it starts with the node's closing token (`}`, `end`, `</div>`),
  so a Rust function shows `fn f() {` then `}`, a Python one only its `def` line.
- **Commands, Emacs's hideshow**: `hs-toggle-hiding` (`C-c @ C-c`) shows the blocks
  hidden after point's line, else hides the block starting on it or the innermost one
  point is in; `hs-hide-all` (`C-c @ C-M-h`) hides every block, those within others too,
  so showing one shows its blocks hidden; `hs-show-all` (`C-c @ C-M-s`). In View →
  folding too. Point in a block hidden goes to its header.
- **Measured** (`plugins/syntax/benches/indent.rs`), in a 2 900- and a 29 000-line Rust
  file: hiding every block takes 9 and 87 ms, after looking up the captured nodes by id
  instead of in a list (210 ms); a key typed with every block hidden costs 5 and 37 µs,
  the folds' lines moved by the lines an edit added instead of looked up one by one
  (3.3 ms); telling a frame the rows takes 60 ns, one allocation.
- **Marks in the gutter**: `▾` right of the number of a line a block starts on, `▸` of
  one whose block is hidden, which also shows `⋯ N lines` after its text; a click on
  either runs `hs-toggle-hiding` on that line. Finding the blocks of the lines drawn
  takes 0.15–0.24 ms a frame, read anew each frame: a cache would show the blocks of the
  tree last edited, not of the one parsed since. Without an indent query the lists are
  looked for among the children touching the lines only: 63 µs in a list of 200 000
  items, 42 ms when every child was visited.
- **Lines drawn around a fold** are decorated in runs, one on each side, not line by
  line as lines far apart once were: 0.4 ms a frame instead of 1.5 ms with a block of
  500 lines folded mid-window.
- **Kept with the session** (`state/session.json`): each file's folded lines, hidden
  again when it is opened with as many lines as then.
