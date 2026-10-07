# 0050 — Run configurations and consoles

Status: accepted, 2026-10-07 (issue #5, part 1: running; debugging is part 2)

## Decision

- **A run plugin, `plugins/run`**, like IntelliJ's run configurations. The debugger
  (DAP, part 2) starts the same configurations, so it builds on this plugin.
- **Configurations are kept per project in the config folder** (user's choice):
  `~/.config/remacs/run/<root name>-<hash>.toml`, one `[[configurations]]` table each,
  read again whenever the file changes. Which one each project runs is state:
  `state/run.json`. When workspaces come (#14), the file moves to the workspace.

  ```toml
  [[configurations]]
  name = "api"
  kind = "cargo"          # cargo | npm | node | executable
  package = "api"         # cargo: package, bin or example, command, cargo-args
  cwd = "."               # relative to the project root; else the kind's own
  args = ["--port", "3000"]
  env = { RUST_LOG = "debug" }
  env-file = ".env"       # relative to the working directory
  before-launch = ["pnpm --filter ui build"]   # shell commands; one failing stops the run
  ```

- **Entry points have a ▶ in the gutter**: a Cargo binary's or example's `fn main`
  (`src/main.rs`, `src/bin/`, `examples/`, `[[bin]]` paths), a whole `.js`/`.ts` file
  (not declarations, tests or configs), each script of a `package.json`. A click opens a
  menu: Run, Edit configuration. The gutter got a column for marks, made by any plugin
  through `Registry::gutters`; part 2 adds breakpoints there.
- **The first run of an entry point saves its configuration at once** (user's choice:
  permanent, not IntelliJ's temporary ones), appended to the run file so that hand edits
  and comments stay. The same kind (the same binary, script or file) finds the saved one.
- **Two ways to run a configuration's kind**:
  - cargo: `cargo <command> -p <package> [--bin|--example]`, from the Cargo workspace's
    root;
  - npm scripts with the package manager whose lock file is nearest (npm, pnpm, yarn,
    bun), from the `package.json`'s directory;
  - TypeScript on Node when it strips types itself (22.6 on, with the flag before 22.18
    and 23.6), else `tsx` from `node_modules` (user's choice); `run.toml` can force
    either.
- **A console buffer per configuration**, `*run: <name>*`, read-only, reused by every
  run of it; a run going on is stopped first and the next one starts once it ended.
  Steps (`before-launch`, then the program) run in turn, each in its own process group so
  that Stop ends what it started too: SIGTERM, then SIGKILL after `stop-timeout`.
  stdout and stderr share one pipe, interleaved as a terminal shows them. No terminal
  emulator (user's choice): the program is not on a TTY, so `run.toml` sets
  `CARGO_TERM_COLOR`, `FORCE_COLOR` and `CLICOLOR_FORCE` for colors.
- **Output is drawn as a terminal would**: SGR colors (16, 256, RGB), bold, dim,
  italic, underline as `Face::Console` spans; a carriage return redraws the line; other
  escape codes are dropped. Output is appended unrecorded for undo
  (`Text::append_output`), and a console keeps the newest `console-lines`.
- **`file:line:column`s and URLs are links**, found in the lines on screen: RET follows
  one (relative paths from the program's directory, then the project root), TAB and
  S-TAB move between them. RET elsewhere sends a line to the program; `C-c C-d` closes
  its input, `C-c C-c` stops it, `M-1`…`M-9` show the other runs' consoles.
- **The status pill shows the chosen configuration** and how its run is going (idle,
  running with a clock, exited with its code, stopped); a click chooses another. A
  console's title bar has Stop and Rerun, from `Registry::text_controls`.
- **Keys as in IntelliJ** (user's choice), any of them rebound in the new `keys.toml`:
  `C-r` run the chosen configuration (in a console: that one again), `C-R` run the entry
  point at point, `M-R` choose a configuration (saved ones, then what the project can run
  that has none yet: npm scripts, Cargo binaries), `s-<f2>` stop. Function keys `<f1>` to
  `<f12>` are new key names.

## Later

- Part 2: debugging the same configurations through DAP (codelldb, js-debug), adapters
  downloaded the way #10 will download language servers.
- Tests as entry points, attaching to a running process.
- A form for editing a configuration; for now it is edited in its file.
