# 0069 — Vite+: oxlint and oxfmt with vite.config.ts, fix-all, `vp check`

Status: accepted, 2026-10-18

## Decision

- **oxlint and oxfmt read Vite+'s `vite.config.ts` through `VP_VERSION`.** Vite+ keeps
  their settings in its `lint` and `fmt` blocks, which both tools, CLI and `--lsp`, read
  only with `VP_VERSION` set; `-c vite.config.ts` fails for both, and `VP_VERSION` in a
  plain Vite project makes them fail on its config. A catalog entry's `env` is added to
  the server's environment for projects where `env-when` holds (a `when` condition;
  without it, every project): `VP_VERSION = "1"` for projects depending on `vite-plus`.
- **oxlint serves projects depending on `vite-plus`** as well as those with its config
  or dependency. A `vite.config.*` alone is no sign: plain Vite projects have one.
- **oxfmt is a server** (`oxfmt --lsp`, the project's own copy first) for TS, JS, JSX,
  TSX, JSON, CSS and YAML of projects with `vite-plus`, `oxfmt` or `.oxfmtrc*`.
- **`formats-first`: an add-on server formats its files ahead of the language's own**,
  which formats only where no such server serves the file: oxfmt, and biome, rather than
  tsc. Embedded code keeps its own server, as it is formatted by regions. oxfmt formats
  whole files only; a region says "oxfmt does not format a region".
- **`fix-all` (Edit ▸ Fix All)** asks the buffer's servers for `source.fixAll` actions on
  the whole buffer (`context.only`, filtered again for servers that ignore it), carries
  out the only one, or offers several in the code actions popup; "Nothing to fix"
  without. oxlint offers safe fixes only (`fixKind` default); suggestions such as
  removing `debugger` stay quick fixes.
- **Compile offers `vp check`** in a project whose `package.json` names `vite-plus`: a
  compile default may require its file to `contain` a text.

## Consequences

- Only the project's own copies read `vite.config.ts`: a global npm oxlint or oxfmt
  looks for `vite-plus` beside itself and fails to load the config. oxfmt installs from
  npm: the native GitHub build has no `--lsp`. npm links them into `node_modules/.bin`;
  pnpm, which links only direct dependencies there, leaves them in
  `node_modules/vite-plus/node_modules/.bin`, also looked in for the packages a server's
  `when` names. That directory goes first on the server's `PATH`, as npx does, or oxlint
  finds no `tsgolint` for type-aware rules.
- A project's root never is the home directory or above: a stray `~/package.json` made
  every Git-less project under it one project rooted at `~`, with no `VP_VERSION`, and
  oxlint and tailwindcss scanned the whole home (oxlint answered `initialize` in 30-48 s).

## Later

- vitest tests (ADR 0053) through `vp test run <file> -t <name>`, debugging with
  `--inspect-brk`.
