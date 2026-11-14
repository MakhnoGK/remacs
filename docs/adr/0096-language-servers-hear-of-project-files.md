# 0096 — Language servers hear of their project's files changing

Status: accepted, 2026-11-09 (issue #186)

## Decision

- **remacs watches for the servers.** Clients say they take
  `workspace/didChangeWatchedFiles` registrations (`dynamicRegistration`), so servers
  that would watch the project themselves, like rust-analyzer and gopls, leave it to
  remacs instead of each running a watcher of their own. A server's registrations are
  kept with its client, by id, until it unregisters them; `RelativePattern`s are
  joined to their base. Globs follow LSP: `*` within a name, `**` across directories,
  `{a,b}` either.
- **Config files are listed per server** in the catalog, as `config-files`: globs from
  the project root, such as `rust-analyzer.toml`, `**/Cargo.{toml,lock}`, `.clangd`,
  `{ts,js}config*.json`, `**/go.{mod,work}`, `tailwind.config.*`, eslint's and biome's.
  `lsp.toml` changes them like any other key; a bad glob is reported as `lsp.toml`'s
  error.
- **Told if registered, else restarted.** A changed file a server registered for, for
  the kind of change it asked for (created, changed, deleted), is told to it in one
  `didChangeWatchedFiles` per burst. A changed config file it did not register for
  restarts it for that project root alone, like `lsp-restart` but sparing its other
  projects, and the echo area says which file did it. A file both registered and
  listed is only told.
- **One watch per project root**, shared by every server running there and dropped once
  none runs, so an idle editor watches nothing. The watch's thread sleeps until a file
  changes, gathers changes until `watch-delay` (`lsp.toml`, 200 ms) passes without one,
  or five delays at most while a build keeps writing, then routes them by root on that
  thread: notifications go straight to the server's writer, and only a restart wakes
  the editor. Paths are matched under the root as the server named it, even where the
  system reports it otherwise (macOS's `/private/var`, symlinked roots).

## Later

- A config file appearing in a project where its server is not running, like a first
  `tailwind.config.js`, starts nothing until the next `lsp-restart`: only running
  servers' roots are watched, and the `when` a server was declined for is not looked at
  again.
- Files outside the project root are never told, though a server registers for them,
  like rust-analyzer for a path dependency in `../shared`; that server watched them
  itself before it left watching to remacs. Watching the fixed start of each
  registered glob that lies outside the root would cover them.
- Changes are matched against every registered glob; a server registering for
  `node_modules` hears of a whole `npm install` in one notification, as with any
  client without exclusions.
