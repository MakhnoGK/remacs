# 0062 — Zooming the editor's text

Status: accepted, 2026-10-14 (issue #43)

## Decision

- **Only the window buffer's text zooms**: its lines, line numbers, gutter marks and the
  cursor. Prompts, popups, tables, outlines, the pane, terminals, menus and the bottom row
  keep `ui.toml` `[font]` size, as `font::code()` stays the configured font; the view
  draws text with `EditorView::text_font`, the code font at the zoomed size.
- **Keys and commands** in the frame plugin: `text-scale-increase` (⌘=, ⌘+),
  `text-scale-decrease` (⌘-), `text-scale-reset` (⌘0), in the View menu as Zoom In, Zoom
  Out, Actual Size. They ask the view through `FrameRequest::TextSize`, as the size is
  the view's, like the window's.
- **⌘ with the wheel or trackpad** zooms instead of scrolling, up larger: one step per
  notch of a plain wheel, or per `wheel-step` pixels of a trackpad or a smooth-scrolling
  mouse (which sends pixels, ~100 a notch). What is left over toward one direction is
  dropped on turning back.
- **Settings**, `ui.toml` `[zoom]`: `step` (points), `min-size`, `max-size`, `wheel-step`.
  A step never goes past the limits, and never away from a configured size already past
  one.
- **The status pill shows the zoom while there is one**, over text: the size in percent
  of the configured one (e.g. `115%`), before the position; a click runs
  `text-scale-reset` (user, 2026-10-14).
- **The zoom is an offset over the configured size**, kept when `ui.toml` changes and not
  kept across restarts. Rows keep their ratio to the size, rounded to whole pixels.

## Consequences

- Text and screens are measured apart (`metrics`, `screen_metrics`), each cached by the
  font it was measured from.
