# 0035 — Glass: translucent window and surfaces

Status: accepted, 2026-10-05 (issue #3)

## Decision

- **Window (board 22)**: the window is `WindowBackgroundAppearance::Blurred` and the
  editor's root is tinted `glass::WINDOW_TINT` (`rgba(0,0,0,0.8)`) instead of opaque black.
  The corners are the system's; a titled window's radius cannot be set.
- **gpui's blur view uses `UnderWindowBackground`**: on macOS 27 the `Selection` material
  gpui chose is a flat fill with no backdrop blur, which gpui then strips, leaving the
  desktop sharp. gpui still removes the material's tint and saturation, so the blur is plain.
- **Backdrop blur in the vendored gpui** (`Styled::backdrop_filter(radius, saturation)`,
  `Window::paint_backdrop_filter`): CSS `backdrop-filter: blur() saturate()` for what the
  window itself draws; the desktop under the window is the window server's. A new
  `BackdropBlur` primitive ends the render pass, copies the drawable around its bounds (so
  the layer is no longer `framebufferOnly`), blurs the copy with MPS `MPSImageGaussianBlur`
  (edges clamped) and draws it back saturated and clipped to the rounded bounds. It takes its
  draw order from its bounds grown by 3σ, so it comes after everything it samples.
- **Fades into the glass** (`Window::paint_fade(bounds, amount, color)`): mixes what is
  drawn toward a color by a gradient's alpha. Scrolled lists dissolve into the glass at
  their edges instead of being covered with black (ADR 0038 replaced the header and
  footer fades).
- **Both mix with dual-source blending**: `dst = color · m + dst · (1 − m)`; a plain quad
  cannot, since "over" always raises alpha.
- **gpui blends alpha "over"** (`1 − src_alpha`) instead of adding it up, which made any
  translucent element opaque on a translucent window and hid the desktop under it.
- **Shadows are not drawn under their element**, as in CSS: a translucent surface would
  show its own shadow. A backdrop filter is painted before the element's shadows, so it does
  not blur them in either.
- **Floating surfaces (boards 23, 24)**: prompts, completion and documentation are
  `widgets::glass_surface`: `glass::SURFACE` over a 30px blur at 150% saturation, a
  `SURFACE_BORDER`, 14px corners for prompts and 12px for surfaces at point. What sits in
  them is translucent too: fields `FIELD`, buttons and key caps `CONTROL`, the field's label
  `CHIP`, the selected row `SELECTION`. The scrim under a prompt is `SCRIM` (25%).
- **Panes, modal and pill (boards 25–27)**: panes in a buffer (git status, dired preview)
  are not blurred but washed white on the window's glass (`PANE`), with code in `WELL`s and
  diff lines in translucent `ADDED`/`REMOVED`. A question that must be answered is a
  `widgets::modal_surface`, thicker (`MODAL`, 40px blur) over a `MODAL_SCRIM` that blurs
  the editor by 3px. The status pill is `PILL` over a 20px blur. The opaque greys these
  replace are gone from the theme.
- **The boards give the look, not the structure**: elements and features are remacs's own;
  e.g. find file keeps its single column, without the board's preview.
- **The conflict view** (board 29, 2026-10-07): the code sits in a `WELL`; each conflict
  is a card of `SURFACE` glass over it, blurred 20px (`CARD_*`), its sides tinted over
  the glass, its buttons `CONTROL`s with key caps sunk into them. The rebase banner is a
  little stronger amber on glass.

## Consequences

- Each blur is a render pass break, a copy and an MPS blur per frame; its two textures are
  allocated per blur and frame.
- Changes to the vendored gpui: `scene.rs`, `window.rs`, `style.rs`, `styled.rs`,
  `build.rs`, `platform/mac/{metal_renderer.rs, shaders.metal, window.rs}`.

## Open

- The boards' inset top highlight on surfaces: gpui has no inset shadows.
