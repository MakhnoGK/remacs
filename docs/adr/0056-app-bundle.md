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
- **Version** (from the tag since 2026-10-20, #86): `CFBundleShortVersionString` is
  `git describe --tags --match 'v[0-9]*'` without its `v` — `0.1.0-alpha.1` on the tag,
  `0.1.0-alpha.1-5-gabc1234` five commits past it, the workspace's before the first tag;
  `CFBundleVersion` the number of commits; `RemacsCommit` the short hash, `-dirty` when
  built from a tree with changes. The xtask builds the binary with the same two as
  `REMACS_VERSION` and `REMACS_COMMIT`, so `remacs --version`, the About panel (version,
  commit in parentheses) and crash reports say them; a plain `cargo build` says the
  workspace's version and "unknown commit". Outside a git checkout (a source archive,
  as Homebrew builds from), `REMACS_VERSION` set in the environment is the version and
  `REMACS_COMMIT` the commit, with `CFBundleVersion` 0.
- **`remacs` on the command line** is a script in `~/.local/bin` written by the install:
  it starts the installed binary with the shell's environment and directory, detached
  (`nohup`), so closing the terminal tab does not quit it. One window per call, as
  before. `remacs --version` runs the binary in the foreground and prints its version.
- **Logs and crash reports** (#81): unless stderr is a terminal (`cargo run`), the app
  points its own stdout and stderr at `~/.config/remacs/state/remacs.log`, so what it
  prints is kept whether Finder, the Dock or the launcher started it; past `log-size`
  bytes a start moves it to `remacs.log.old`. A panic hook, on every thread, writes the
  message, the backtrace, the version and the commit to `state/crashes/<time>.log`
  before the default hook prints the panic. The next start says in the echo area that a
  report exists and where (reports newer than `crashes/.shown`); a panic on a worker
  thread (git monitor, LSP readers…), which leaves remacs running, is said at once.
  `logs.toml`: `log-size`, `crash-reports` kept.
- **Third-party notices** (#87): `cargo xtask bundle` puts `THIRD-PARTY-NOTICES` in the
  bundle's Resources: the license texts of every crate built in, the tree-sitter
  grammars among them, each text once with the crates under it, written by
  [cargo-about](https://github.com/EmbarkStudios/cargo-about) (`cargo install
  cargo-about --locked --features cli`) from `crates/app/bundle/about.toml` and
  `notices.hbs`, offline, from the crates' own license files. It takes half a minute, so
  `target/bundle/THIRD-PARTY-NOTICES` is kept and made again only when `Cargo.lock` or
  those two files are newer. Help → Third-Party Notices (`view-third-party-notices`)
  opens it read-only. remacs's own crates (`publish = false`) and its icons are not in
  it.
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

## Releases

Tagged `v0.1.0-alpha.1`, `v0.1.0-alpha.2`, …, then `v0.1.0`. `CHANGELOG.md` is written
by `cargo xtask changelog [version]` from the history: a section per tag of the `feat`,
`fix` and `perf` subjects since the one before, the untagged commits under `version` (or
"Unreleased"); the other types change nothing people see. It is regenerated whole, so
fix a wording in the commit, not in the file.

Work lands on `dev`, the default branch; `main` holds releases only, so it moves once
per release, not with every change. Cutting a release:

1. On a branch from `dev`, `cargo xtask changelog 0.1.0-alpha.2`, read it, and open a
   pull request `chore: release v0.1.0-alpha.2` into `dev`; the owner merges it.
2. A pull request from `dev` into `main`, merged with a merge commit (a ruleset allows
   no other), so `main` keeps `dev`'s commits and the two never diverge.

The rest is `.github/workflows/release.yml`, on the push to `main` (#115); a version
already tagged does nothing:

3. The version is `CHANGELOG.md`'s newest section. The tag `v0.1.0-alpha.2` goes on the
   merge's second parent, the release commit in `dev`, which `main` contains too, so
   `git describe` finds it from both branches.
4. A GitHub release, a prerelease while the version has a `-`, its notes that section:
   `remacs-0.1.0-alpha.2.tar.gz` (`git archive` of the tag) and its SLSA build
   provenance from `actions/attest` as `remacs-0.1.0-alpha.2.tar.gz.sigstore.json`
   (`gh attestation verify remacs-0.1.0-alpha.2.tar.gz -R MakhnoGK/remacs`).
5. A pull request `bump-remacs-0.1.0-alpha.2` in the tap with that archive's URL and
   SHA-256, opened with the secret `TAP_TOKEN` (a fine-grained token for the tap alone,
   contents and pull requests: the workflow's own token cannot write to another
   repository, and a pull request it opens runs no CI). The tap's test-bot builds the
   bottle; its `auto-publish` then runs `brew pr-pull`, which publishes it. `brew upgrade
   remacs` installs it some 15 minutes after the merge.

By hand, if a step fails: tag and `gh release create v<version> --prerelease
--verify-tag`; `brew bump-formula-pr --url <the archive> makhnogk/remacs/remacs`, then the
tap's `brew pr-pull` workflow with the pull request's number, as `MakhnoGK`.

## Homebrew (2026-10-24)

Without a Developer ID (#82) a downloaded app is quarantined and Gatekeeper blocks it,
so a cask of a .dmg would not open; an app Homebrew installs from a bottle has no
quarantine and opens. remacs is a formula in our tap
[`MakhnoGK/homebrew-remacs`](https://github.com/MakhnoGK/homebrew-remacs): it builds the
tag's source archive with `cargo xtask bundle` (`REMACS_VERSION` from the formula,
`REMACS_COMMIT` `homebrew`), installs `Remacs.app` in its prefix and a `remacs` launcher
like the one above; `head` builds `dev`.

- **Bottles** are built by the tap's `brew test-bot` and published by `brew pr-pull` into
  the tap's releases: arm64 only, on macOS 15, which every later macOS takes. Homebrew no
  longer bottles `rust` for macOS 14, so there test-bot skips the formula; the formula
  needs Sequoia, and macOS 14 builds from source with `cargo xtask install`.
- **Linux** (since 0.1.0-alpha.2): the formula installs the Linux bundle (ADR 0089) as
  its prefix, without `install.sh`, the menu entry running `opt_lib/remacs/remacs`; it
  depends on `libxcb` and `libxkbcommon`. Its x86_64 bottle is built on `ubuntu-24.04`,
  as `brew doctor` fails on 22.04's glibc 2.35. The menu entry and icons show once the
  session's `XDG_DATA_DIRS` has Homebrew's `share`, as `brew info remacs` says.
- **Build dependencies**: `rust`, `cargo-about`, `cmake` (wasmtime's C API builds with
  it), Xcode for the Metal shaders. `cargo fetch --locked` runs first, as the notices
  read every crate in the lock file offline.
- No scheduled autobump in the tap: a pull request opened by a workflow's token runs no
  CI; remacs's release opens the bump instead (step 5 above).
- Since 0.1.0-alpha.2 the formula builds the release's attested archive, not GitHub's
  generated tag archive.

## Later

- A self-signed identity instead of ad hoc, if macOS asks for folder access again after
  every install.
