# 0036 — Problems at point

Status: accepted, 2026-10-05 (design board 07b)

## Decision

- **Point, not the mouse**: the problems under point show in a bubble above the line,
  their messages in line with the problem's text, most severe first, with who reported
  each. Below the line when there is more room.
- **After a pause**: point resting 500 ms on a problem after a command that left the text
  as it was shows it. Typing never does, so the bubble stays out of the way while editing;
  moving through problems shows nothing until point stops.
- **Right away after navigation**: `M-n`/`M-p`, `M-g n`/`M-g p` and choosing a row in
  the Problems list show the bubble at once. Their messages are no longer echoed in the
  status pill.
- **Any command hides it**, and point moved by the mouse. Documentation, a menu or a
  prompt at point go first: the bubble waits while one is open.
- **Board 07b's look on glass**: an info-tinted surface, `quoted` parts of messages as
  code chips. TypeScript and eslint quote code with `'`: in messages without backticks,
  single quotes that are not apostrophes inside a word count as backticks.

## Core additions

- `Editor::refresh_after`/`take_refresh_at`: a plugin asks the UI to refresh after a
  delay, like an Emacs idle timer; the UI keeps one timer, the earliest, and hooks ask
  again on every refresh until their moment comes.
- `Popup::notes` (`Note`: icon, message, source); `Popup` derives `Default`.
