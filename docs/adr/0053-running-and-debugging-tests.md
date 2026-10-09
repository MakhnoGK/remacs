# 0053 — Running and debugging tests

Status: accepted, 2026-10-09 (follows 0050 and 0051)

## Decision

- **Configurations are still written in the run file** (user's choice), no form. The
  chooser (`M-R`) ends with "New configuration…": it appends a commented example to the
  run file and visits it there.
- **`kind = "cargo"` runs tests** (user's choice, rather than a shell kind):

  ```toml
  [[configurations]]
  name = "store tests"
  kind = "cargo"
  command = "test"
  package = "api"
  lib = true              # or bin = "…", test = "<file of tests/>", example = "…"
  filter = "store::tests::"
  args = ["--nocapture"]  # the test binary's own
  ```

  `cargo test -p api --lib store::tests:: -- --nocapture`. `Kind::Cargo` holds a
  `Cargo` struct whose `selection()` both running and debugging use.
- **Debugging `cargo test`** builds with `cargo test --no-run --message-format=json` and
  debugs the test binary with `[filter] args`, from the package's directory as `cargo
  test` runs it. Several binaries built (no target named) are offered in the minibuffer;
  the session waits, and Stop ends it.
- **▶ beside tests**: each `#[test]` function (any attribute whose last segment is `test`
  or `rstest`: `#[tokio::test]`…) and each module holding tests, found with
  tree-sitter-rust. The target comes from the file (`src/` → the library if there is
  one, else the binary; `src/bin/`; `tests/<name>` → `--test <name>`), the module path
  from the file's path and the modules around. A function runs as `filter =
  "path::to::it"`, `args = ["--exact"]`; a module as `filter = "path::to::it::"`.
- **Tests run without saving a configuration** (user's choice): the last one run is the
  project's temporary configuration, in memory until quit, chosen like a saved one
  (`C-r` reruns it) and listed in the chooser as "Temporary". Saving it ("Save
  configuration…" in the ▶ menu, `C-e` in the chooser) makes it permanent. A saved
  configuration of the same kind is used instead of a temporary one.
- `run-at-point` (`C-R`) and `debug-at-point` inside a test's body run that test: the
  innermost entry point around point.

## Later

- Tests of other languages (vitest, jest).
- Test results as a tree, rerunning the failed ones.
