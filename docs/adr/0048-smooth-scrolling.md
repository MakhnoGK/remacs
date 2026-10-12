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
  `ui.toml` `scroll.animation-duration` milliseconds (150 by default, 0 jumps). A notch
  goes `scroll.lines-per-notch` lines or rows (3 by default; #25).
- **A wheel in pixels glides too** (#25): drivers like Logi Options+ send a wheel's
  notches as precise pixel deltas, ~100 px every ~20 ms, which gpui could not tell from
  a trackpad's. The vendored gpui sets `ScrollWheelEvent::from_wheel` when an event has
  neither a gesture's phase nor momentum's; those glide as far as the driver says,
  without `lines-per-notch`.
- **Targets add up into one glide** (#25): a new target sets off from where the glide is
  at that moment, at the speed it reached, and comes to rest on the target (a cubic
  from that speed; from rest, or turning back, the ease-out's own speed). Notches faster
  than the animation neither jolt nor stall a frame.
- **Long jumps start a screen away**, so M-> in a large file does not race past pages.
  Another buffer is shown at once.
- **Text, tables and outlines alike**, with the one setting: lines and rows are drawn
  from the one partly scrolled past, clipped, one more drawn at the bottom. An outline's
  rows differ in height; the wheel counts them as table rows.
- **An outline's history glides too** (#25): its plugin hands over commits only from the
  first one scrolled to, so the view glides a position counted from that commit and,
  each frame, moves the plugin's first commit by the whole commits it passed, drawing
  only the fraction left. Gliding up passes below zero first, so the commit above is
  handed over before it is drawn.
