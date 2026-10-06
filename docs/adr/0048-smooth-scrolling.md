# 0048 — Smooth scrolling

Status: accepted, 2026-10-06

## Decision

- **The core stays in whole lines**: `Viewport::first_line` and `first_column` decide
  where point may be, as before. The view draws from a fractional position
  (`SmoothScroll`) that follows them, one for lines or rows and one for columns.
- **Trackpads move the content by pixels**, momentum included, since macOS sends the
  momentum as more scroll events. What is short of a whole line stays a fraction past
  the first line until the viewport moves for another reason.
- **Everything else glides**: wheel notches, commands (C-v, M-v, recentering, point
  leaving the window, point near a side) and dragging past an edge ease out over
  `ui.toml` `scroll.animation-duration` milliseconds (150 by default, 0 jumps). A new
  target restarts the glide from where the content is drawn.
- **Long jumps start a screen away**, so M-> in a large file does not race past pages.
  Another buffer is shown at once.
- **Text, tables and outlines alike**, with the one setting: lines and rows are drawn
  from the one partly scrolled past, clipped, one more drawn at the bottom. An outline's
  rows differ in height; the wheel counts them as table rows.
- **An outline's history scrolls by pixels but does not glide**: its plugin hands over
  commits only from the first one scrolled to, so there is nothing to draw above it
  while gliding up.
