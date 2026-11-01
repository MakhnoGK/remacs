# 0091 — Run and debug without writing configurations

Status: accepted, 2026-11-01 (issue #215, stage 1: task runners)

## Decision

- **A configuration is found, never written from scratch** (owner's rule): whatever a
  project can run is offered ready, with a ▶ beside it and in the chooser (`M-R`); the
  run file is for editing what was found. Each focused stack adds its own discovery
  (#197, #200, #201, #206, #209, #213) through `entry_points`, which feeds both.
- **Clean room** (owner's rule): designs come from our own requirements, not from
  other IDEs' features, code or docs. Another tool's file is read only as a format,
  from its public specification, to take over a project's existing configurations.
- **Stages**, one pull request each: (1) task runners; (2) "New configuration…" built
  from what is at point, every field written out; (3) VS Code's `launch.json` and
  `tasks.json`, JetBrains' `.run/*.run.xml` and Zed's tasks imported; (4) a schema for
  the run file; (5) discovery over the whole file index, measured; (6) a fixture per
  stack that runs and debugs its entry point with no typing.
- **Stage 1: Makefiles and justfiles.** Two kinds, `kind = "make"` and `kind = "just"`,
  each a `file` (relative to the project root) and its `task`:

  ```toml
  [[configurations]]
  name = "tools: build"
  kind = "make"
  file = "tools/Makefile"
  task = "build"
  args = ["DEBUG=1"]
  ```

  run as `make -f Makefile build DEBUG=1` and `just --justfile justfile <recipe> <args>`
  from the file's directory. The file is always named, so a directory holding both a
  `GNUmakefile` and a `Makefile` runs the one listed.
- **Targets are read from the text**, not from `make -pRrq` or `just --summary`:
  discovery runs while the chooser opens and must not start processes. Make: targets of
  rule lines, without special (`.PHONY`), pattern (`%.o`), variable (`$(BIN)`) and file
  targets (any `.` or `/`), names that are only dependencies, `define` blocks and
  continued lines; included makefiles are not followed. just: recipes, without `_…`,
  `[private]`, settings, aliases and variables. Checked against `make -pRrq` on five
  real Makefiles and by hand on four justfiles from crates.io.
- Neither kind can be debugged; Debug says so.

## Later

- `deno.json` tasks, once a JSONC reader exists (`deno.jsonc` is common).
