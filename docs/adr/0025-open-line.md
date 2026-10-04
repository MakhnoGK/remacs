# 0025 — Opening a line below or above

Status: accepted, 2026-10-04 (issue #1 editing experience, step 2)

## Decision

- **`s-RET` (`open-line-below`)** starts a line under the current one from anywhere on it,
  as `RET` at its end would: a level deeper after an unclosed bracket (ADR 0024).
- **`s-S-RET` (`open-line-above`)** starts a line over the current one at its level, a
  level deeper when the current line starts with a closer, so the new line lands inside
  the block.
- The current line is never broken; point goes to the new line. Emacs's `C-o` (which
  breaks the line and keeps point) is a different command and stays unbound.
