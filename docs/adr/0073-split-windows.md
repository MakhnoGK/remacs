# 0073 — Split windows

Status: accepted, 2026-10-20 (issue #90)

## Decision

- **Windows are a binary tree in core** (`crates/core/src/windows.rs`): a leaf is a
  window, a split has an axis, a ratio and two children. `C-x 2` (`split-window-below`)
  and `C-x 3` (`split-window-right`) halve the selected window; the new one, below or on
  the right, shows the same buffer at the same point and scroll, and the old one stays
  selected, as in Emacs. `C-x 0` (`delete-window`) gives the space to its sibling and
  selects the window selected before it; `C-x 1` (`delete-other-windows`) keeps the
  selected one. A split that would leave either half under `windows.toml` `min-lines`
  or `min-columns` is refused.
- **The selected window is what the single window was**: `buffers[0]` and
  `Editor::viewport`, so commands and plugins that act on the window buffer act on the
  selected window unchanged. Another window keeps its buffer (by `BufferId`, as names
  change) and its viewport; its point lives in its buffer (`Buffer` window points) and
  edits move it like the mark, so two windows on one buffer keep points of their own.
- **Other windows are drawn as if selected** (`Editor::display_window`, then
  `end_display`): their buffer, point and viewport take the selected one's place for the
  time it takes to draw them, without touching the buffer order or running hooks, and
  `buffer_has_keys` is false meanwhile. Providers (tables, outlines, gutters, controls,
  decorations) need no window argument; this is what Emacs redisplay does too.
- **Display hooks** (`Registry::display_hooks`) keep what is drawn of a buffer current —
  syntax trees, the Git status, changed lines — in the windows not selected: after the
  post-command hooks, once per buffer only they show, each displayed in turn. The same
  hooks stay among the post-command hooks for the selected window, so its order of
  hooks is as it was.
- **Killing a buffer** shows, in each other window that showed it, the most recent buffer
  no window shows, else the most recent one. `C-x k` on the selected window's buffer
  while another window shows it too only takes it off the selected window (user,
  2026-10-20), like Emacs `bury-buffer`.
- **One pane, under all windows** (user, 2026-10-20): the pane spans the frame's width
  under the windows and `C-x 1` keeps it. A terminal in a window takes only its window,
  and the pane stays under it, where before a terminal in the window hid the pane.
- **`C-x o` goes through the windows in layout order, then the pane** if one is shown,
  and back to the first window; with one window it is the toggle it was. While the pane
  has the keys, the window commands act on the selected window and give it the keys.
- **A click selects the window under it; the wheel scrolls the window under the pointer**
  without selecting it.
- **A terminal has one size**: the selected window's if it shows it, else the first
  window's in layout order that does, else the pane's. Other places that show it say it
  is shown elsewhere; a click there selects the window that has it.
- **Showing a file from a list goes to another window** (user, 2026-10-20), like Emacs
  `display-buffer`: a debugger stopping, `next-error` and links in consoles show the file
  in a window already showing it, else the most recent window but the selected one,
  without selecting it; with one window they replace its buffer as before
  (`Editor::display_file`).
- **Each window names its buffer** (user, 2026-10-20): windows at the top in the title
  bar over them, the others in a strip of `windows.toml` `header-height` at their top;
  the selected window's name is bright, the others' dim, and what the others show is
  dimmed a little (`inactive-opacity`, user, 2026-10-20). A 1px line divides windows.
  In other windows the cursor is hollow and there is no region or current line. Popups
  belong to the selected window; prompts, the scrim and the bottom row to the frame.
  Tabs and toolbars of a window's buffer go in its header.
- **`C-x 4 b` (`switch-to-buffer-other-window`) and `C-x 4 f`
  (`find-file-other-window`)** prompt as `C-x b` and `C-x C-f` do, then select the
  window selected before the selected one and show the answer there, as Emacs
  `pop-to-buffer` does; a lone window is split first, along its longer side on screen (a
  cell counted twice as tall as wide), else along the other.
- **Resizing moves a split's ratio.** `C-x ^` (`enlarge-window`), `C-x }`
  (`enlarge-window-horizontally`) and `C-x {` (`shrink-window-horizontally`) move the
  selected window's edge by `windows.toml` `resize-step` lines or columns (there is no
  prefix argument yet), in its nearest split along the axis, or a split further up
  when that one has no room; every window keeps `min-lines` or `min-columns`. Cells are
  turned into a share of the frame through the selected window's viewport, so headers
  and gutters make them approximate. Dragging the 1px divider (a strip of
  `divider-grab-width` grabs it) sets the ratio of its split under the mouse, with the
  same minimums. `C-x +` (`balance-windows`) gives windows stacked along each axis equal
  room, like Emacs.
- **Commands** in the `files` plugin, beside `other-window` and the buffer commands
  (`frame` depends on `dired`, which depends on `files`); menu items in Window › windows:
  Split Below, Split Right, Delete Window, Delete Other Windows (while there are
  others), Other Window, Switch to Buffer in Other Window…, Open in Other Window…, and
  Taller, Wider, Narrower, Balance Windows (while there are others).

## Later

- Other `*-other-window` commands (`C-x 4 d`, `C-x 4 0`); `C-x 4 b` reusing a window
  that already shows the buffer.
- `shrink-window`, and a prefix argument for the resize commands.
- Restoring the layout with the session (#91).
- LSP requests for what other windows show (inlay hints, semantic tokens of their lines).
