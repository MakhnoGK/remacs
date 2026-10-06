# 0043 — Horizontal scrolling of long lines

Status: accepted, 2026-10-06

## Decision

- **The whole text scrolls sideways to keep point in view** (`Viewport::first_column`,
  like Emacs `window-hscroll` with `auto-hscroll-mode`), in display columns, so lines
  indented with tabs and with spaces stay aligned.
- **Before point reaches an edge**: point keeps `SIDE_MARGIN_COLUMNS` (7) columns from
  either side (`Viewport::column_margin`, like Emacs `hscroll-margin`; Zed keeps 5, Vim
  users set `sidescrolloff` to 5–8). The left margin applies only while scrolled.
- **As little as needed**: one column per column point moves past a margin, not a jump to
  the middle as Emacs `hscroll-step` 0 or Vim `sidescroll` 0 do.
- **Recomputed while drawing**: only the view knows tab stops and how many columns fit.
  Switching buffers resets the scroll so a short buffer is not left scrolled.
- **Fades at the sides, as in ADR 0038**: per line, on the left when it has text scrolled
  past, on the right when it goes on past the edge, toward what the line is drawn on (the
  glass, or the current line's highlight). The fade is narrower than the margin, so the
  character under point never fades.
- **Only what is in view is laid out**: `display_line::slice_line` reads a line up to the
  columns on screen, so a megabyte line scrolled to its end costs a scan, not a layout.
- **Trackpad and wheel scroll sideways too**, along the swipe's main direction only, so a
  swipe that is not quite straight does not drift. Point stays where it is and may leave
  the view, as in VS Code, until the next key or click brings it back
  (`EditorView::scrolled_sideways`). The scroll stops once no drawn line goes on past the
  right edge.
