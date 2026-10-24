# 0003 — Platform and UI

Status: accepted, 2026-10-04

## Decision

- macOS only; Linux since ADR 0089.
- UI on gpui 0.2.2 from crates.io, `default-features = false` + `font-kit`
  (without it macOS gets a no-op text system and renders no text).
- Only visible lines are laid out (`uniform_list`); long lines are clipped before layout.
- Design tokens live in `remacs-theme`, free of UI dependencies.

## Metal shaders

Xcode's Metal Toolchain is not installed, so the app enables gpui's `runtime_shaders`
(feature `runtime-shaders`, on by default). For release builds install it with
`xcodebuild -downloadComponent MetalToolchain` and build with `--no-default-features`.
