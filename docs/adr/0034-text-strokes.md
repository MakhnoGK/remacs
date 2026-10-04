# 0034 — Thin and thick text strokes

Status: accepted, 2026-10-04 (issue #11)

## Decision

- **Two strokes, thick by default**: `gpui::TextStrokes::{Thin, Thick}`, set once with
  `gpui::set_text_strokes` before the window opens. It applies to every font, code and UI.
- **Thick is Core Graphics font smoothing**, which iTerm2 turns off for "thin strokes".
  Smoothing emboldens only on an opaque context, so a glyph is drawn white on a black RGB
  bitmap and one channel kept as its coverage (Monaco 13 at 2x: ~31% more ink). Its
  raster bounds grow by a device pixel on each side so the bolder edges are not clipped.
  Thin is gpui's own alpha-only rendering. The "light" smoothing style (16) draws the same
  as the default one on current macOS, so there is no third weight. Emoji are unaffected.
- **gpui is vendored** in `vendor/gpui` (0.2.2) and patched in through `[patch.crates-io]`:
  glyph rasterization is private to gpui (`PlatformTextSystem` is `pub(crate)`).

## Consequences

- Changes to the vendored gpui: `text_system.rs` (the setting),
  `platform/mac/text_system.rs` (rasterization), `taffy.rs` (two float literals typed, as
  a path dependency's warnings are no longer capped). Upgrading gpui means re-applying them.
- Changing the strokes at runtime would leave already-rasterized glyphs in the atlas as
  they were; it is set at startup only.

## Open

- The strokes are a constant in `remacs-app`; they move to the config once there is one.
