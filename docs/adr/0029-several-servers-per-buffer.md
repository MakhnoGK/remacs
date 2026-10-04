# 0029 — Several servers per buffer

Status: accepted, 2026-10-04 (lint and format, step 1)

## Decision

- **A buffer's file is open on every server for its mode**, in the order `SERVERS` lists
  them, so linters such as oxlint and eslint serve beside the language's own server.
  Supersedes "one server per buffer" in ADR 0006; still one process per server and root.
- **The first server answers questions**: completion, documentation and definitions.
  Linters are listed after the language's server.
- **Code actions come from all of them**: each is asked, and the menu shows once every
  server answered, in the servers' order; an action is carried out by the server that
  offered it.
- **Diagnostics per checker**: `diagnostics::publish` and `clear` take the checker's
  name (the server's), and a report replaces only that checker's problems. The buffer has
  no diagnostics once no checker is left.
- A server that failed to start is left out of the comparison, so buffers are not
  re-attached after every command.

## Open

- rust-analyzer sometimes answered `codeAction/resolve` with the action unchanged
  ("The code action does nothing"); fixed in ADR 0031.
