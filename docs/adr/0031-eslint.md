# 0031 — eslint

Status: accepted, 2026-10-04 (lint and format, step 3)

## Decision

- **eslint runs through VS Code's server** (`vscode-eslint-language-server`, from
  vscode-langservers-extracted) beside tsc and oxlint (ADR 0029, 0030): warm, so linting
  does not pay Node's start-up per check. It reports diagnostics only when asked (pull).
- **The project's eslint first**: the server loads the eslint library from the project's
  `node_modules`, else the global one (`packageManager: npm` lets it find that). The
  server itself is also taken from `node_modules/.bin` when the project has it.
- **Only for projects with an eslint config**: flat `eslint.config.*` or legacy
  `.eslintrc*` (`LanguageServer::required_files`); without one eslint only fails.
- **Configuration**: the server asks for VS Code's settings with
  `workspace/configuration` and fails on `null`, so servers may carry the JSON to answer
  (`LanguageServer::configuration`). eslint gets VS Code's defaults, with fixes on save
  and formatting off.
- Quick fixes, disabling a rule and "Fix all auto-fixable problems" come with the other
  servers' code actions (`C-c C-a`).

## Resolving while rust-analyzer loads

- While it loads the project, rust-analyzer answers `codeAction/resolve` with the action
  as sent: no edit, `data` still there. Such an answer, given while the server is not
  settled, is resolved again once it is (`experimental/serverStatus`), as dired's
  renames are (ADR 0014).
