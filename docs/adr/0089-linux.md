# 0089 — Linux

Status: accepted, 2026-10-25 (issue #37)

remacs was macOS only (ADR 0003). The first Linux build is the smallest one that runs:
what macOS alone has is turned off there, not rewritten. The desktop it is checked on is
Arch Linux with KDE Plasma on Wayland.

## Decision

- **gpui's Wayland and X11 windows** on Linux (`remacs-app` turns on gpui's `wayland` and
  `x11` features there), drawn by blade over Vulkan. A Vulkan driver is required.
- **Opaque glass**: only the Metal renderer blurs (ADR 0035) and fades (ADR 0038). On
  Linux the window is opaque, `glass` settings are ignored, and blade skips the blur and
  fade primitives.
- **The desktop's fonts**: Monaco and SF Pro Text, the defaults, are replaced by what
  `fc-match` gives for `monospace` and `sans-serif`. Families someone set themselves are
  kept.
- **Alt is Meta**; `[keyboard] meta-option` is macOS's. Thin and thick strokes (ADR 0034)
  are drawn as the font's outlines.
- **No menu bar**: gpui keeps the menus on Linux without drawing them. Commands run from
  `M-x` and their keys. About opens a prompt in the window.
- **Downloads by target**: in the tools catalog (ADR 0063), `{target}` is
  `<cpu>-unknown-linux-gnu` on Linux, so rust-analyzer, ruff, ty, oxlint and the like
  download as on macOS, and codelldb takes its Linux build. An asset named for macOS
  (`darwin`, `mac`, `osx`) is not downloaded there: that tool must be on `PATH`.
  Checksums use `sha256sum`, since Arch keeps Perl's `shasum` off `PATH`. tombi builds
  Linux for musl only, so its `{target}` download fails there.
- **"system" theme** reads the desktop's color scheme from the settings portal
  (`busctl`), as gpui does once its window is open.
- **System calls**: the Trash is freedesktop's (the `trash` crate), an exclusive rename is
  `renameat2(RENAME_NOREPLACE)`, links open with `xdg-open`, the fallback shell is
  `/bin/sh`.
- **xattr 0.2.3 is vendored** (`vendor/xattr`): gpui's http client pulls it in, and it
  names `libc::ENOATTR`, which libc no longer defines on Linux.
- **CI** builds, tests and lints on `ubuntu-latest` too.

## Releases (2026-10-25)

- **A prebuilt archive** next to the source one in each GitHub release:
  `remacs-<version>-x86_64-linux.tar.gz`, built by `release.yml`'s `linux` job from the
  tag on `ubuntu-22.04`, so it needs glibc 2.35 or later, with its build provenance
  (`.sigstore.json`). No AUR package yet: AUR had closed registration; `remacs-bin`
  from this archive comes once it opens. No Flatpak (its sandbox keeps language
  servers, terminals and the shell's `PATH` out) nor AppImage.
- **`cargo xtask bundle`** on Linux makes `target/bundle/remacs-<version>-<cpu>-linux`,
  laid out as a prefix: `lib/remacs/remacs` and its `THIRD-PARTY-NOTICES` (Help's
  notices are read next to the binary), `bin/remacs`, the launcher that starts it
  apart from the terminal as on macOS, `share/applications/io.github.makhnogk.remacs.desktop`,
  the hicolor icons, and `install.sh`.
- **`install.sh [prefix]`** copies them under `~/.local` by default, replacing
  `lib/remacs` whole by a rename; the menu entry runs the binary by its full path, as the
  desktop's session may not have `~/.local/bin` on its `PATH`. `cargo xtask install` runs
  it; there is no `rollback` on Linux.
- **The icon** is macOS's, rendered once at 16–512 px into
  `crates/app/bundle/linux/icons` (16 and 32 from their heavier masters), as Linux has
  no `sips`. The window's app id, `io.github.makhnogk.remacs`, is the menu entry's name,
  so Wayland desktops show that icon for the window; X11 gets it as `WM_CLASS`.
- **The menu entry** offers remacs for `text/plain`, not folders, so that it never
  becomes what opens a folder in the file manager.
- **Homebrew** on Linux installs the same bundle from the tap's formula, bottled for
  x86_64 (ADR 0056).

## Later

A menu bar drawn in the window, blur and fades in blade's WGSL, strokes on cosmic-text,
Linux assets for the catalog's macOS-only entries and tombi's musl build, an AUR package,
an arm64 archive.
