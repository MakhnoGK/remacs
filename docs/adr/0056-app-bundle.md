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

## Later

- Started from Finder or the Dock, the app has launchd's short `PATH`: take the login
  shell's environment once at startup (`$SHELL -l -i -c env`), as VS Code and Zed do.
- Files and folders opened from Finder or dropped on the Dock icon (`on_open_urls`,
  `CFBundleDocumentTypes`).
- A self-signed identity instead of ad hoc, if macOS asks for folder access again after
  every install.
