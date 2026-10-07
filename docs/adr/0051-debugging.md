# 0051 — Debugging and downloaded tools

Status: accepted, 2026-10-07 (issue #5, part 2: debugging; part 1 is ADR 0050)

## Decision

- **Tools are downloaded on first use, `plugins/tools`**: a catalog of programs remacs
  runs but does not ship (codelldb, js-debug now; language servers come with #10), each
  from its GitHub releases. The asset for this Mac is downloaded with curl, checked
  against the release's sha256 digest, unpacked next to its final directory and only
  then moved in: `~/.config/remacs/cache/tools/<name>/<version>/` (ADR 0042), older
  versions removed. `tools.toml` pins a version or names a program of one's own. `C-c t`
  shows the Tools panel (install, update, reinstall, cancel); the status pill shows a
  download going on. Other plugins ask with `remacs_tools::ensure`, which calls back
  once the program is there.
- **Debugging is a way of running, `plugins/debug`**: the run plugin got a `Program`
  trait and a `Starter`, so a debug session drives the same console, Stop, Rerun and
  status pill as a run. Cargo configurations are built first (`cargo build
  --message-format=json-render-diagnostics`, its diagnostics in the console) and their
  executable debugged with codelldb, over stdio; executables likewise; Node and npm
  ones with js-debug, over TCP, answering its `startDebugging` requests with a
  connection per process it debugs. The program's output comes as DAP output events;
  its input is not passed on. `debug.toml`'s `[env]` goes under the run's variables;
  by default `MTL_HUD_ENABLED=0`, since Metal's performance HUD showed while debugging
  from a release build.
- **Keys as in IntelliJ** (user's choice), rebindable in `keys.toml`: `S-<f9>` debug the
  chosen configuration, `C-D` the entry point at point (IntelliJ's ⌃D is `delete-char`
  here), `<f9>` resume, `<f8>` step over, `<f7>` step into, `S-<f8>` step out,
  `M-<f9>` pause, `s-<f8>` toggle a breakpoint, `s-S-<f8>` edit the line's breakpoint
  (else list all), `M-<f8>` evaluate, `C-x o` keys to the pane and back, `C-c f`/`v`/`w`
  its columns. The entry point menu has Debug.
- **Breakpoints** are drawn in the gutter (plain, conditional, log, disabled, not
  verified), a click beside a line adds one (`Registry::gutter_click`), and the paused
  line's arrow goes over them. They are kept in `state/breakpoints.json` and move with
  their lines: line-wise, since a minimal diff of a typed newline often starts inside
  the next line's indentation. Conditions, hit counts and log messages are edited in a
  prompt; the list enables, visits, edits and deletes them.
- **The debug pane** (`Registry::panes`, a core `Pane`) sits under the text while a
  session debugs: a toolbar of the session's state and step buttons, then threads and
  frames, the selected frame's variables (the first scope at the top, others as
  headings; changed values tagged), and watches. While a column has the keys, typed
  characters do not reach the text (`Registry::typing_blockers`). Library frames, of
  code outside the project, are faint and passed over when choosing the frame shown.
- **Inline values** (`Registry::line_extras`): the paused line gets a band, and the
  lines above it, up to `debug.toml`'s `inline-value-lines`, the values of the local
  variables they declare (`let`, `const`, `var`, `for`, parameters).
- **Evaluate** (`M-<f8>`) prints the expression and its value in the console. codelldb
  runs with `consoleMode` `evaluate`, so input is an expression, not an LLDB command.

## Later

- Attaching to a running process, tests as entry points.
- A console of its own for the debugger with tabs (output, logpoints, errors) as on the
  board; the program's stdin under the debugger.
- Language servers in the tool catalog (#10).
