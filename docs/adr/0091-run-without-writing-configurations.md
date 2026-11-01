# 0091 — Run and debug without writing configurations

Status: accepted, 2026-11-01 (issue #215, stage 1: task runners; stage 2: new
configurations; stage 3: the run file's schema; stage 4: discovery over the whole
index)

## Decision

- **A configuration is found, never written from scratch** (owner's rule): whatever a
  project can run is offered ready, with a ▶ beside it and in the chooser (`M-R`); the
  run file is for editing what was found. Each focused stack adds its own discovery
  (#197, #200, #201, #206, #209, #213) through `entry_points`, which feeds both.
- **Clean room** (owner's rule): designs come from our own requirements, not from
  other IDEs' features, code or docs. Other tools' run configurations are not imported.
- **Stages**, one pull request each: (1) task runners; (2) "New configuration…" built
  from what is at point, every field written out; (3) a schema for the run file; (4)
  discovery over the whole file index, measured; (5) a fixture per stack that runs and
  debugs its entry point with no typing.
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

- **Stage 2: "New configuration…" is made, not written.** It saves a configuration of
  what runs at point (the entry point the ▶ would run), else of the chooser's first
  entry, else the project's compile command as `kind = "shell"`, under a free name
  even when the project runs that kind already, so that a variant only needs editing.
  `env-file = ".env"` is set when the program's working directory has one.
- **Every configuration saved lists what it leaves out**, as comments under its table:
  each optional field of its kind and of every kind, with an example and what it does,
  ahead of any sub-table so that a line uncommented stays its own:

  ```toml
  [[configurations]]
  name = "api"
  kind = "cargo"
  package = "api"
  # command = "test"  # "run" when left out; "test" runs tests
  # args = []  # given to the program
  # env-file = ".env"  # a dotenv file, relative to the working directory
  ```

- **Stage 3: the run file names its schema.** `run.schema.json` is derived from the
  configuration types (schemars), written beside the run files whenever one is read or
  added and differs, and named on the first line, `#:schema ./run.schema.json`, which
  taplo and tombi both follow: completion of fields and kinds, the types' doc comments
  as hover, and errors for a wrong kind, value or type. A run file written before gets
  the line on its next addition. The schema is JSON Schema draft 7, the draft taplo
  checks against: in 2020-12 a `$ref` beside the `kind` constant is ignored by it, and
  every kind of task matched every other. tombi also warns of keys no kind has (a
  misspelled `evn-file`); taplo cannot, as kinds are alternatives of one table. Checked
  with taplo 0.10 and tombi 1.9.8 on a run file the store wrote, before and after
  uncommenting every optional field.

- **Stage 4: the chooser finds everything, off the main thread.** The cap of 200
  candidate files is gone: a monorepo's packages past the first 200 were never offered.
  Each project has one search for what it can run, on a thread of its own, kept while
  the chooser is closed: the chooser lists the saved configurations at once and the
  rest when the search is done ("Looking through the project…" until then); opened
  again it lists everything at once. A new search starts only when the file index
  changes and none is going on, so quick openings start no more. A search that fails
  says so in the chooser, and runs again once the index changes. Each entry's command
  line is found once per search, not on every key typed, a search reads each
  `Cargo.toml` once, and a workspace's root is looked for only for a file with a
  `main` or tests. Measured with `cargo bench -p remacs-run --bench discovery` on a
  monorepo of 1000 npm packages of five scripts and 1000 Cargo binaries (6001 rows),
  median of 10, and checked again by a second reviewer:

  | | before | after |
  |---|---|---|
  | `discover` | 90 ms | 63 ms |
  | main thread, opening the chooser the first time | 219 ms | 15 µs, then 7 ms as the search ends 137 ms later |
  | main thread, opening it again | 219 ms | 1.9 ms |
  | main thread, a key typed and deleted | 279 ms | 2.6 ms |

  "Before" is without the cap, which bounded the old code only by leaving packages
  out. The first opening was measured before searches were kept per project; the bench
  now measures opening again. The gutter's search on a pause in typing (`--bench
  entry_points`) stays 3.1 µs.

## Later

- `deno.json` tasks, once a JSONC reader exists (`deno.jsonc` is common).
