# 0033 — Formatting on save

Status: accepted, 2026-10-04 (lint and format, step 5)

## Decision

- **Off by default**, on from the start with `on-save = true` in `format.toml`
  (ADR 0042); `format-on-save-mode` turns it on or off for all buffers, for the session.
- **After the save, like apheleia**: saving never waits for a formatter. Each save of a
  file-visiting buffer, by any command, asks to format the whole buffer; the edits are
  made and the file saved again, unless the text changed meanwhile, which the next save
  formats. Fixes from eslint or oxlint are not made on save; they stay on `C-c C-a`.
- **A save is told by the save count** (`Text::saves`), not by the buffer turning
  unmodified, which undoing back to the saved text also does. The plugin's own second
  save is not formatted again.
- **Failures**: backends say whether nothing formats the file (`Failure::Unsupported`:
  no formatter, a server that cannot format, `.prettierignore`) or formatting failed
  (`Failure::Failed`, e.g. a syntax error). After a save only failures are shown.

## Consequences

- A formatted save writes the file twice; watchers, such as dev servers, may see both.
