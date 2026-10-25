# 0065 — Compile: shell commands with their output's locations

Status: accepted, 2026-10-16

## Decision

- **`compile` as in Emacs M-x compile**, in the run plugin, on its consoles: a shell
  command line run from the project root, so pipes, `&&`, globs and variables work
  where a run configuration runs one program. Bound to ⌘F9 (IntelliJ's Build), in
  Run ▸ Compile….
- **The prompt offers the project's commands, newest first**, the last one selected so
  that RET runs it again; what is typed comes first among the candidates, so RET runs it
  as typed unless another is chosen; TAB puts the selected one in the input to edit.
  Commands are kept per project in `state/compile.json`, `run.toml` [compile] `history`
  of them. A project that compiled nothing yet is offered the command of the first of
  [compile] `defaults` whose file is at its root (Cargo.toml → `cargo build`, …).
- **One compile console per project**, `*compile: <directory>*`, a run console in all
  else: colors, links, stdin, Stop. A new compile stops the one going on first. The
  command is a `kind = "shell"` configuration that is not saved and not chosen; the
  status pill shows it while the console is shown. Its end is said in the echo area
  ("Compilation finished", "exited abnormally with code 2"), the console perhaps hidden.
- **The shell is `run.toml` [compile] `shell`, else `$SHELL`**, given the line with `-c`;
  the console shows the line itself, not the shell's arguments.
- **`recompile`** (⌘⇧F9, `g` in any console, as `C-r` there) runs the project's compile
  again, else its last command, else asks.
- **The directory is the nearest project root, or Dired's** (2026-10-25): from a file,
  the nearest directory within the project holding the file of one of [compile]
  `defaults` (Cargo.toml, package.json, …), else the project root; from a Dired buffer,
  its directory; from a console, its run's. Commands and the console are that
  directory's, so a monorepo's packages each keep their own.
- **`compile-next-error` / `compile-previous-error`** (M-g M-n / M-g M-p, also C-x `)
  visit the output's `file:line` links in turn, URLs skipped, like Emacs next-error: in
  a console from point, elsewhere from the location visited last in the project's
  compile console, else in the console of the latest run. `M-g n` / `M-g p` stay the
  buffer's diagnostics.
- **`kind = "shell"` run configurations** (`command`, plus `args`, `env`, `cwd`, …) run
  the same way, for commands worth keeping; they cannot be debugged.

## Consequences

- A `file:line` in the command line itself, echoed at the top of the console, counts as
  a location.
- Errors are not told from warnings or notes: every location is visited.

## Later

- Compile output's locations as diagnostics (gutter, Problems).
- Saving a compile command as a run configuration from the prompt.
