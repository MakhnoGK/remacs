# 0047 — Selecting text with the mouse

Status: accepted, 2026-10-06

## Decision

- **Clicks in a row pick the unit**: one puts point, two select a word, three a line with
  its newline. Dragging extends the region by the same unit, always keeping what the
  press selected (`MouseSelection` in core, so it is tested without a window).
- **A word is a run of one class on its line**: identifier characters (letters, digits,
  `_`), spaces, or punctuation, so `snake_case` is one word and `);` another. Past a
  line's end, the run before it.
- **A plain click keeps the mark**, as in Emacs; it only deactivates the region. A drag
  sets the mark where it started.
- **Shift extends**: from the active region's mark, or else from point, by the clicks'
  unit.
- **Dragging past the top or bottom scrolls** a line every `ui.toml`
  `mouse.drag-scroll-interval` milliseconds (40 by default) while the mouse stays there,
  including over the faded margin lines and outside the window; the wheel also scrolls
  and extends during a drag. Sideways, point near an edge already scrolls the text
  (ADR 0043).
- **The mouse is followed through window-wide listeners** registered only while the
  button is held, since an element's own handlers stop at its bounds.
