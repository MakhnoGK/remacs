# 0056 — App bundle and install

Status: accepted, 2026-10-10 (icon: board AppIcon, direction A "Parens and cursor")

## Decision

- **The installed app is a snapshot.** remacs is developed in remacs, so building the
  workspace, or breaking the build, must not touch the editor in use. `cargo xtask
  install` builds the release, bundles `Remacs.app` and puts it in `~/Applications` (no
  sudo); the app it replaces is kept as `Remacs.previous.app`, and `cargo xtask rollback`
  swaps the two. The new app is copied next to its place first, so installing is two
  renames on one volume.
- **`xtask`, our own crate**, std only: no cargo-bundle or cargo-packager for a plist, an
  icon and a signature. `cargo xtask bundle` builds `target/bundle/Remacs.app` alone.
- **Bundle**: `crates/app/bundle/Info.plist` (id `io.github.makhnogk.remacs`, developer
  tools category); the icon is made with `sips` and `iconutil` from three masters, 16 and
  32 px drawn heavier than the 1024 one. Signed ad hoc (`codesign -s -`), enough for the
  Mac that built it.
- **Version**: `CFBundleShortVersionString` is the workspace's; `CFBundleVersion` the
  number of commits; `RemacsCommit` the short hash, `-dirty` when installed from a tree
  with changes.
- **`remacs` on the command line** is a script in `~/.local/bin` written by the install:
  it starts the installed binary with the shell's environment and directory, detached
  (`nohup`), so closing the terminal tab does not quit it; what it prints goes to
  `~/.config/remacs/state/remacs.log`. One window per call, as before.
- **Metal shaders are compiled at build time** (Xcode's Metal Toolchain:
  `xcodebuild -downloadComponent MetalToolchain`); the `runtime-shaders` feature
  compiles them at startup instead, for a Mac without it.

- **The login shell's environment.** Started from Finder or the Dock, the app has
  launchd's `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`), so language servers, `cargo` and
  `git` are not found. Before the plugins start, remacs runs `$SHELL -l -i` once and takes
  what `env -0` prints between two markers (read up to the end marker, not to the end of
  output, which a daemon started by the startup files may hold open), leaving out what
  the shell or a terminal sets (`SHLVL`, `PWD`, `TERM`…), and starts in the home
  directory rather than `/`. `environment.toml`: `from-login-shell = "auto"` (unless
  started from a terminal, i.e. `TERM` is set) | `"always"` | `"never"`; `timeout`, past
  which the environment stays as it is and the echo area says why.
- **`shell` in `environment.toml`** (added 2026-10-10): a path or a name in place of the
  account's login shell, for a terminal that runs another one. The account's zsh put a
  pnpm `tsc` 5 ahead of Homebrew's `tsc` 7 on `PATH`, where the user's fish does not, so
  TypeScript had no server in the app. `SHELL` is then set to it, so terminals run it
  too. A language server that exits is now reported in the echo area with the last
  lines of its stderr, and not started again until `lsp-restart` (no key; it offers the
  window buffer's servers first), its `lsp.toml` settings change, or remacs starts again.
- **Opened from Finder**: "Open With", a file or folder dropped on the Dock icon, or
  `open -a`. `CFBundleDocumentTypes` lists any file (`public.text`, `public.data`) and
  folders, both at rank Alternate, so remacs is offered but never made the default.
  gpui's `on_open_urls` hands the `file://` URLs over a channel to the window, which may
  not be open yet; each opens in it as on the command line, a folder in dired.

## Later

- A self-signed identity instead of ad hoc, if macOS asks for folder access again after
  every install.
