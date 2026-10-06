# 0042 — Config

Status: accepted, 2026-10-06 (issue #2)

## Decision

- **One home for everything remacs keeps**: `$XDG_CONFIG_HOME/remacs`, else
  `~/.config/remacs` (`remacs_config::home`). Nothing is written next to projects or
  elsewhere in `$HOME`. Planned layout:
  - `config/`: settings;
  - `cache/projects/<root hash>/`, `cache/dirs/`: caches, e.g. the file index;
  - `state/`: what lasts between sessions, e.g. recent files, `format-on-save-mode`;
  - `servers/` (#10), `grammars/`, `plugins/` (WASM).
- **Settings split into files, one section each**: `config/ui.toml` is the `ui` section.
  A section is a type implementing `remacs_config::Section` (serde, defaults for what is
  left out, `deny_unknown_fields` to catch typos, `check` for values out of range). Files
  are parsed only when their section is asked for.
- **Errors never stop remacs**: a section that does not parse or check falls back to its
  defaults, and the first error is shown in the echo area as `ui.toml:3: …`.
- **`ui.toml` first**, for the code font only; UI font settings come later:

  ```toml
  [font]
  family = "Monaco"
  size = 13
  line-height = 1.7   # times size, rounded to whole pixels
  strokes = "thick"   # thick | thin

  [cursor]
  height = "line"     # line: the whole row; font: ascent and descent, centered
  ```

- **The code font is process-wide** (`remacs_theme::font::code`, `set_code`): its family
  is the monospace font everywhere, its size and line height are the window buffer's.
  Surfaces (prompts, tables, popups) keep their own sizes, `font::MONO_SIZE` and
  `font::LINE_HEIGHT`. A global rather than state passed down, as many render helpers
  have no view or context.
- **The editor's cursor is a block drawn under the text**, not a highlight background, so
  its height can differ from the line's; the character under it keeps its inverted color.

## Consequences

- Line height is a multiplier, so changing the size keeps the proportions.
- Metrics are re-measured whenever the code font differs from the one they came from.
- Settings are read at startup; changes need a restart until the config is watched.

## Open

- Reload on change (`notify` on `config/`); strokes may still need a restart (ADR 0034).
- Move `format-on-save-mode` to `state/` and the file index to `cache/`.
- Plugins register their own sections through the registry.
