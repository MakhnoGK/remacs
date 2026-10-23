# 0078 — Following the system's appearance

Status: accepted, 2026-10-23 (issue #96)

## Decision

- **`ui.toml` `[theme] name = "system"`** shows `light` (default `modus-operandi`) while
  macOS is light and `dark` (default `modus-vivendi`) while it is dark; `select-theme`
  offers `system` first. `system` is a reserved name, never a theme file.
- **The theme follows macOS instead of macOS the theme**: under `system` the
  application's appearance is left nil, so its windows' appearance is the system's, and
  gpui's window-appearance observer sees each switch; the view then reads the settings
  again for the new appearance, with no restart. Under a named theme the theme still sets
  the appearance macOS draws its own parts in (ADR 0066), and the observer does nothing.
- **At start and on a settings change** the appearance is read from the user's defaults
  (`AppleInterfaceStyle`), which a forced application appearance does not hide; on a
  switch, from the window, which has changed by then.
