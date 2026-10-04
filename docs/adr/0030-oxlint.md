# 0030 — oxlint

Status: accepted, 2026-10-04 (lint and format, step 2)

## Decision

- **oxlint runs as a language server** (`oxlint --lsp`) beside tsc for TypeScript,
  JavaScript, TSX and JSX (ADR 0029): warm, so linting costs milliseconds. It publishes
  diagnostics on its own and offers quick fixes with their edits.
- **Only for projects that use it**: a server may require files, looked for from the
  file's directory up to the Git work tree (`LanguageServer::required_files`). oxlint
  requires `.oxlintrc.json`, `.oxlintrc.jsonc` or `node_modules/.bin/oxlint`, so a global
  oxlint does not lint every JavaScript project. Directories found lacking them are
  remembered per server, so they are looked for once.
- **The project's own copy first** (`LanguageServer::node_modules`): `node_modules/.bin`
  of the project root or above, as npx does, else the command on `PATH`. tsc keeps the
  global one: a project's own `tsc` is mostly TypeScript 5, which has no `--lsp`.
- Root markers `package.json` and `.oxlintrc.json`; oxlint finds nested configs itself.

## Open

- A config added while remacs runs is seen after a restart.
- Fix all (`source.fixAll.oxc`) is not offered yet: only on command, later.
