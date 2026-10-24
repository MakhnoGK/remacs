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
  Checksums use `sha256sum`, since Arch keeps Perl's `shasum` off `PATH`.
- **System calls**: the Trash is freedesktop's (the `trash` crate), an exclusive rename is
  `renameat2(RENAME_NOREPLACE)`, links open with `xdg-open`, the fallback shell is
  `/bin/sh`.
- **xattr 0.2.3 is vendored** (`vendor/xattr`): gpui's http client pulls it in, and it
  names `libc::ENOATTR`, which libc no longer defines on Linux.
- **CI** builds, tests and lints on `ubuntu-latest` too.

## Later

A menu bar drawn in the window, blur and fades in blade's WGSL, strokes on cosmic-text,
Linux assets for the catalog's macOS-only entries, a `.desktop` file and a package.
