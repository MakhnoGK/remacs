# 0042 — Config

Status: accepted, 2026-10-06 (issue #2)

## Decision

- **One home for everything remacs keeps**: `$XDG_CONFIG_HOME/remacs`, else
  `~/.config/remacs` (`remacs_config::home`). Nothing is written next to projects or
  elsewhere in `$HOME`. Planned layout:
  - `config/`: settings;
  - `cache/projects/<root hash>/`, `cache/dirs/`: caches, e.g. the file index;
  - `state/`: what lasts between sessions, e.g. recent files, prompt history;
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
- **Plugins read their own sections**: the `Editor` holds the settings
  (`set_settings`), and runs `Registry::settings_hooks` whenever they are read; a hook
  takes its section with `Editor::settings_section`, which shows an error and returns
  the defaults when the section is wrong. The app reads `ui` itself. `set_settings` then
  refreshes, so plugins act on new settings without waiting for a command.
- **`lsp.toml` changes the built-in servers by name**, what it leaves out stays built in:

  ```toml
  [servers.rust-analyzer]
  command = "~/bin/rust-analyzer"   # a path, or a name on PATH
  args = []
  initialization-options = { check = { command = "clippy" } }
  configuration = {}                # answered for workspace/configuration

  [servers.eslint]
  enabled = false
  ```

  A server whose settings changed is restarted: its clients are dropped (the process
  exits as its input closes), its files closed and their diagnostics cleared, and a
  failure to start is forgotten; the refresh then starts it with the new settings where
  it is wanted. Unknown server names are reported with the known ones.
- **Settings give modes their starting state**: `format.toml` has `on-save = true` to
  start with `format-on-save-mode` on. The command still toggles it for the session; the
  toggle holds until the section itself changes, not when another file does.
- **The minibuffer history lasts across sessions**, like Emacs savehist:
  `state/history.json`, by completion category, newest first, 100 answers each. Read at
  startup after this session's, written after each answer through a temporary file. A
  prompt marked `private` (the kill ring's) adds nothing, so copied text never reaches
  the disk. A broken file is reported and left as it is.
- **Projects' caches**: `cache/projects/<root name>-<FNV-1a of the root path>/`, given
  to plugins by `Editor::project_cache_dir` once the app calls `keep_caches_in` (tests do
  not). First user: `files`, the file index, one path per line. A session lists it at once
  while the scan runs, then the scan replaces it. It is written after each full scan only:
  rewriting up to 100 000 paths for every created file is not worth it, and the next
  session's scan finds such changes.

## Consequences

- Line height is a multiplier, so changing the size keeps the proportions.
- Metrics are re-measured whenever the code font differs from the one they came from.
- **Settings apply as soon as a file changes**: `config/` (created at startup) is watched
  with `notify`, changes settle for 100 ms, then the section is read again and applied,
  strokes included (ADR 0034). The directory is watched, not the files, as editors often
  save by renaming over them.

## Open

- More `state/`, e.g. recent files once remacs has them; `cache/dirs/` once something
  needs it.
- `lsp.toml` servers for languages remacs has no built-in server for; servers downloaded
  into `servers/` (#10).
