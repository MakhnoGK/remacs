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

- Changes to the vendored gpui: `text_system.rs` (the setting, `thick` in
  `RenderGlyphParams`, and `set_family_weight`, which shifts the code font's weights by
  `font.weight` when its font is resolved, issue #23), `window.rs` (sets it when painting a glyph),
  `platform/mac/text_system.rs` (rasterization), `taffy.rs` (two float literals typed, as
  a path dependency's warnings are no longer capped), `elements/text.rs` (a truncated
  text is measured again at another width, ADR 0058), `app.rs` and
  `platform/app_menu.rs` (`App::on_validate_menu_action`, ADR 0060), `elements/inert.rs`
  and `window.rs` again (inert elements; backdrop blurs and fades follow the element's
  opacity, ADR 0061). Upgrading gpui means re-applying them.
- The strokes are part of a glyph's atlas key, so changing them at runtime rasterizes the
  glyphs again on the next frame. They are read from `ui.toml` (`font.strokes`), see
  ADR 0042.
