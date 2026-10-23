# 0058 — The echo area and the key hints share one bottom row

Status: accepted, 2026-10-11 (issues #20, #21; design boards 49–53)

## Decision

- **One row along the bottom, three pills that never move each other** (user: the layout
  must not jump while working): the echo area on the left, the view's keys in the middle,
  the status on the right. The keys are centered on the window; how many show depends on
  the window's width only (`hints.side-room` is kept free on each side), never on the
  message. The ones that do not fit go under a `+N` chip, or a `Keys · N` chip when none
  fits; a click on it or `C-h b` (`list-view-keys`; `C-h k` until ADR 0077) lists them
  above the row.
- **A message is one line**, cut with an ellipsis where the keys begin, with the key that
  shows it whole (`C-h e`) when cut. A click on it opens the whole message above it
  (`echo.lines`, then it scrolls): `M-w` copies it, `C-g` or any key or click closes it.
- **Messages have a kind**: `set_message` (info, no mark), `set_done` (green dot),
  `set_error` (red dot and border). Errors returned by commands are errors. Each records
  the command that was running and the time.
- **`*Messages*`** (`C-h e`, `view-echo-area-messages`, plugin `echo`) lists every
  message: time, command, text, later lines under the first, repeats counted as `×N`,
  errors in red. Point on the newest follows newer ones. `q` closes it, `C-c C-k` clears
  it. The core keeps the last `echo.history` messages.
- **A prefix typed** shows as key caps in the echo pill. After `prefix-keys.delay`, a
  panel above the row lists what each next key does, in `prefix-keys.columns`, like Emacs
  `which-key`; a key that is itself a prefix shows its sequence and `…`.
- **Progress stays in the status pill** (user's choice), as a status segment.
- **Settings**, in `ui.toml`: `[echo] lines, history`, `[hints] show, side-room`,
  `[prefix-keys] show, delay, columns`.

## Consequences

- The status pill no longer carries the echo, so a long message cannot push it.
- `text-overflow: ellipsis` works: gpui reused a nowrap text's first measure, made before
  its width was known, so `.truncate()` clipped without the ellipsis. The vendored gpui
  now keeps the width a text was cut to (`elements/text.rs`, `truncate_width`), see
  ADR 0034's list of patches.
- Keys of the opened message (`M-w`, `C-g`) are handled by the view, like copy mode's in
  a key sink, not by keymaps.
- y/n questions stay in the minibuffer; drawing them in the echo pill (board 52) is left
  for later.
