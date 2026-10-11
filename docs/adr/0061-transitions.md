# 0061 — Transitions of panes, prompts and popups

Status: accepted, 2026-10-11 (issue #24)

## Decision

- **Surfaces fade, and may slide, in and out**: the pane under the text (terminal,
  debugger), prompts (the minibuffer, menus from the toolbar, dialogs, the problem list)
  with their scrim, and the popups by the text (completions, documentation, problems).
  The pane rises from below, prompts and popups come down from above, the problem list
  rises from the status pill. Panes switched or one prompt answered by the next do not
  transition: the surface stays, its content changes.
- **Settings**, in `ui.toml` `[transitions]`: `pane-`, `prompt-` and `popup-duration`
  (milliseconds, 0 to 100, 0 off) and `-slide` (pixels, 0 to 32, 0 only fades). Defaults
  90/16, 80/8, 60/0.
- **Only drawing transitions, never input**: a surface coming in takes keys and clicks at
  once; one going out is drawn inert (`gpui::inert`), so the mouse reaches what is under
  it, and keys go where they would without it. Nothing waits for a frame.
- **A surface going out is drawn from its last description** (`transition::Retained`):
  panes and popups are described anew each frame and the view keeps the last one; the
  core hands over a closed prompt (`Editor::take_closed_minibuffer`), dropped once it has
  faded or another prompt opened. Popups are told apart by their plugin
  (`Editor::popup_slots`).
- **Frames are asked for only while something moves**; at rest nothing more is drawn
  than before. The text's layout never animates: a pane's room is taken or given back at
  once, the pane fades over it.

## Consequences

- Backdrop blurs and fades follow the element's opacity in the vendored gpui (they did
  not), so a fading popup's blur and edge fades fade with it (ADR 0034's list).
- A pane going out draws its screen only if the window does not draw the same one, so a
  screen is sized to one place a frame.
- Popups vanish at once when the window stops showing text; they belong to it.
- While a pane slides in, its screen maps the mouse as if it rested, at most `pane-slide`
  pixels off for `pane-duration`.
- The echo area's whole message, the key lists above the bottom row and the prefix keys
  (ADR 0058) do not transition yet.
