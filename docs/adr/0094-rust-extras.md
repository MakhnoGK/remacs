# 0094 — Macro expansion, parent module, doctests, benchmarks, nextest and clippy

Status: accepted, 2026-11-09 (issue #205)

## Decision

- **Parent module and `Cargo.toml` are xref lookups**, like implementations (ADR 0092):
  `xref-find-parent-module` and `xref-find-manifest`, in Go ▸ related, gone to or listed
  and come back from with `M-,`. `xref` names them for any language; `lsp` asks
  rust-analyzer's `experimental/parentModule` and `experimental/openCargoToml`, offered
  only where its `initialize` answer lists them under `experimental`.
- **`expand-macro` shows the expansion read-only**, in `*vec! expanded*`, in the major
  mode of the file it came from, so it is highlighted as Rust; one per macro, replaced
  when expanded again. rust-analyzer never lists `rust-analyzer/expandMacro`, so it is
  offered where `parentModule` is, which only rust-analyzer lists.
- **Doctests: a ▶ beside each code block of a `///` comment** that rustdoc tests (no
  language, `rust`, `no_run`, `should_panic`, `compile_fail`, `edition…`; not `text`
  or `ignore`), in a library's files only, run as `cargo test -p <package> --doc
  <item path>`. The path is rustdoc's: modules, then an `impl`'s type as written
  (`store::Store<T>::write`). rustdoc splits its filters at spaces, so a block cannot be
  picked by the `(line N)` its test name ends in: every block of an item runs together,
  with those of items whose path starts the same.
- **Benchmarks: a ▶ beside each `#[bench]` and `#[divan::bench]` function** and each
  module holding some, found as tests are, and **beside each criterion
  `bench_function("id", …)` call** in `benches/`, run as `cargo bench -p <package>
  --bench <file> <path or id>`; `#[bench]` in a library's tests as `cargo bench --lib`.
  Cargo configurations gain `bench`, the target, as they have `test`. No `--exact`:
  harnesses list a function's runs under it, as divan does by argument.
- **cargo-nextest is chosen at launch**, not stored: a cargo configuration running tests
  runs `cargo nextest run` instead of `cargo test` when `run.toml`'s `nextest` is on
  (the default) and its working directory, the workspace root, has
  `.config/nextest.toml`. Doctests stay on `cargo test`, as nextest cannot run them.
- **rust-analyzer checks with clippy** while `lsp.toml`'s `clippy` is on (the default):
  its initialization options get `check.command = "clippy"` unless they set one, so a
  change restarts it as any changed server does. Whether `cargo clippy --version` runs
  in the project is asked on the thread that writes to the server, before `initialize`,
  never on the main thread; where it does not, as with a toolchain without clippy, the
  command is left out and rust-analyzer checks with `cargo check`.

## Later

- The whole benchmark target from a ▶ beside `criterion_main!` or divan's `main`.
- Doctests of `//!` comments and of items inside functions.
