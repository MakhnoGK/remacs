# 0038 — Fades at the edges of scrolled lists

Status: accepted, 2026-10-05

## Decision

- **Only scrolled content fades, only where there is more**: the text, a table's rows
  (dired), an outline's rows (git status) and its history dissolve at the top when lines
  were scrolled past and at the bottom when more follow. The window's own header and
  footer fades from ADR 0035 are gone: they dimmed the first line of every file and the
  bars, column titles and panes of tables and outlines.
- **`widgets::fade_scrolled` / `edge_fade`**: `Window::paint_fade` over the list's own
  bounds, toward what the list is drawn on, so the background stays as it was: the
  window's glass, or `PANE` over it for an outline's rows (`widgets::over`).
- **Scroll margins** (`Viewport::margins`, like Emacs `scroll-margin`): point stays a line
  away from a faded edge, so the line it is on never fades. A margin applies only while
  there are lines beyond it. Text has a top margin only: its bottom fade is already over
  the lines under the status pill, outside the viewport.
- **Knowing there is more**: text compares the viewport with the line count; a table is
  asked for one row more than fits; an outline is asked for more rows than can fit; the
  history says whether commits were scrolled past (`History::scrolled`) and has more than
  fit.

## Not covered

- Floating surfaces (prompts, completion, documentation) are over a blurred backdrop that
  no single color restores, so their lists do not fade.
