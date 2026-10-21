# 0066 — Themes

Status: accepted, 2026-10-16 (issue #61)

## Decision

- **A theme is data**: a TOML file of every token of the look — `[colors]` (opaque
  `#rrggbb`), `ansi` (16), `[styles]` (bold/italic/underline of the syntax colors),
  `[glass]` (translucent `#rrggbbaa`), `[ink]` and `[shadow]`, plus `name`, `appearance`
  (`light`/`dark`) and `opacity`. Built in, compiled from `crates/theme/themes/`:
  `modus-vivendi` (the colors before themes, unchanged) and `modus-operandi`, its light
  twin (replaced the first light theme, TextMate's Mac Classic, for one family of looks).
- **Chosen in `ui.toml` `[theme] name`**, applied live like the other settings; the
  themes directory `~/.config/remacs/themes` is watched too. `select-theme` (View >
  Theme…) lists the themes and writes that line, keeping the rest of the file, so
  `ui.toml` stays the one place the choice lives. It has no preview: prompts have no hook
  for the selected candidate.
- **A user's theme sets only what it changes**: its table is merged over the theme it
  `inherits`, else over the built-in theme of its `appearance`; a file named like a
  built-in theme changes that theme. Unknown tokens and malformed colors are errors,
  shown like settings errors; the default theme is drawn meanwhile.
- **Tokens are functions reading atomics** (`color::fg_dim()`, `glass::surface()`): one
  relaxed load, no lock, from any thread (terminals' grids read the ANSI colors off the
  main thread). `tokens!` generates each table's struct, slots and readers from one list.
  The default theme is put in place before the first read, so nothing is drawn unthemed.
- **Transparency**:
  - The window's tint is the theme's `bg-main` under `[glass] opacity`. Left out of
    `ui.toml`, the opacity is the theme's: a light tint over a dark desktop turns gray and
    takes contrast from the text, so Modus Operandi's is 0.92 against Modus Vivendi's 0.8.
  - Lines, hovers and tints drawn over the glass with a fixed alpha take `glass::ink(α)`:
    white on dark, black on light. Shadows take `glass::shadow(α)`, whose `strength`
    softens them on light surfaces (0.45). Translucent surfaces (`surface`, `modal`,
    `pill`, `note`, `field`, …) are the theme's own RGBA.
  - The line highlight and the region are RGBA, so a translucent one such as
    `#00000012` is drawn over the glass as it is; fades into the line highlight mix
    it over the tint first. Dual-source fades and backdrop blur are unchanged (ADR 0035).
- **macOS draws its own parts with the theme**: `NSApp.appearance` is Aqua or Dark Aqua
  by the theme's appearance, so menus, the traffic lights and the window's shadow match.

## Consequences

- Colors are no longer constants: nothing can put one in a `const`, and a hardcoded
  `0xRRGGBBAA` in a view is a bug — it is either a token, `ink`/`shadow` with an alpha, or
  a token shifted with an alpha (`color::accent() << 8 | 0x29`).
- Blur radii, saturations and corner radii stay constants in `glass`; floating surfaces'
  blur is not part of a theme.
- Loading TextMate `.tmTheme` files builds on this (issue #62): a tmTheme fills the same
  tokens, the rest derived from its background, foreground and selection.
