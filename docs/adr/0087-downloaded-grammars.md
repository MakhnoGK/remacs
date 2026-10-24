# 0087 — Grammars downloaded as WebAssembly

Status: accepted, 2026-10-24 (issue #102, step 6)

Some grammars have no crate built for the tree-sitter remacs uses (Dockerfile, gitignore,
git-rebase, ADR 0082), and every grammar built in makes the binary bigger. Building one
on a user's Mac would need a C compiler and run its code unsandboxed.

## Decision

- **WebAssembly, run by wasmtime**: tree-sitter's `wasm` feature loads a grammar built to
  `.wasm` and parses with it as with a native one, sandboxed; no compiler is needed. A
  `Grammar`'s language is `Source::Built` (compiled in, as before) or
  `Source::Downloaded { name, release }`. Each parser gets a wasmtime store of its own
  the first time it is given a WebAssembly language; one engine serves the run.
- **Only languages remacs does not build in** (user, 2026-10-24): Dockerfile, Fish,
  `.gitignore` (Git Ignore) and `git-rebase-todo` (Git Rebase). The built-in grammars
  stay native.
- **Built by remacs's CI, from a pinned list**: `plugins/languages/grammars.toml` names
  each grammar's repository and revision. `cargo xtask grammars <dir>` fetches each at its
  revision without its queries, which stay unread (clean room, ADR 0081), and builds it
  with tree-sitter-cli 0.27.1, which fetches the WASI SDK itself, next to its license.
  The Grammars workflow (run by hand) publishes them as the release `grammars-<release>`
  of `MakhnoGK/remacs`, never marked latest, and never replaced: remacs's queries are
  written for what it holds. A new revision or grammar makes a new release, and
  `GRAMMARS_RELEASE` in `grammars.rs` names it; a test keeps the two in step.
- **Queries stay in remacs** (user, 2026-10-24), written from each grammar's node types
  like the others; a new downloadable language therefore still comes with a remacs
  release. Tests load copies of the release's files from `plugins/languages/tests/grammars/`
  and compile every query against them.
- **Downloaded at once, no dialog** (user, 2026-10-24): a file whose grammar is not there
  starts its download through the tools plugin, with its progress in the status line, and
  is highlighted as code without a grammar meanwhile (ADR 0083); the grammar is loaded
  once there, and every file of it is parsed from then on. `syntax.toml`
  `download-grammars = false` turns it off. A grammar is asked for once a run; a failed
  download says why in the echo area. Grammars are tools of their own kind: listed in the
  Tools panel, kept in `~/.config/remacs/cache/tools/tree-sitter-<name>/<release>/`,
  checked against the release's checksum, and bound to their release (`Tool::version`), so
  no newer one is offered as an update.
- Dockerfile's (`docker-langserver`) and Fish's (`fish-lsp`) language servers join the
  catalog, offered as the others (ADR 0063).

## Measured

Release build, tree-sitter 0.27.1 against the same grammar native and as WebAssembly,
the median of full parses, then of a reparse after a one-byte edit:

| Grammar | File | Native | WebAssembly | Edit, native | Edit, WebAssembly |
|---|---|---|---|---|---|
| Rust | 1 MB | 79 ms | 131 ms (1.65×) | 2.9 ms | 3.2 ms (1.07×) |
| Rust | 101 MB | 8.4 s | 14.7 s (1.75×) | 414 ms | 564 ms (1.36×) |
| JSON | 1 MB | 64 ms | 105 ms (1.65×) | 1.65 ms | 1.68 ms (1.02×) |
| JSON | 100 MB | 6.9 s | 11.1 s (1.62×) | 270 ms | 441 ms (1.63×) |

The trees were the same. Under issue #102's bar of 2×, so common languages could move to
downloads later; for now they stay native. Loading a `.wasm`: 2 ms (JSON, 6 KB) to 23 ms
(Rust, 1.1 MB). wasmtime adds 8.9 MB to the binary (69.1 → 78.0 MB).

## Consequences

- `remacs-syntax` depends on `remacs-tools`; `Grammar::language()` is `None` for a
  downloaded grammar not loaded yet, and its queries are compiled once it is.
- `Tool` has `version`, the only release that will do unless `tools.toml` pins another;
  `ToolKind::Grammar` is provided before the servers, so it does not check `tools.toml`'s
  names itself.
- Highlight queries of one's own for a downloaded grammar are read on
  `reload-syntax-queries` after it is loaded, not at start.
