# 0068 — Code of other languages inside a file

Status: accepted, 2026-10-17 (issue #42, stages 1 and 2: highlighting, language at point)

## Decision

- **Injections as layers**, like Helix, Zed and Neovim: a host mode may have an
  injection query (`Injections { host, query }`, `languages::INJECTIONS`,
  `SyntaxPlugin::new(GRAMMARS, INJECTIONS)`). Each `@injection.content` capture is
  parsed with the grammar whose mode is the pattern's `#set! injection.language`,
  ignoring case. All ranges of one language make one tree (`set_included_ranges`), parsed
  on the same parser thread right after the host's, incrementally from its last tree.
- **HTML**: a script's `type` or `lang` picks its language — none, `module` or
  `text/javascript` → JavaScript; `lang="ts"` or `text/typescript` → TypeScript;
  `importmap`, `speculationrules` and `…json` → JSON; any other type stays text.
  `<style>` → CSS. `style="…"` and `on…="…"` attributes are not injected yet.
- **The injection query runs again only where the text or the host tree changed**
  (`Tree::changed_ranges` plus the edit since the layers were found): walking a 240k-node
  page takes 8 ms, more than the parse. Each range remembers what the query matched to
  find it (its owner, e.g. the script's tag), so an edit to a `type` finds it again.
- Highlighting asks each layer for the visible bytes; the host leaves injected code
  unhighlighted, so the spans are merged by position.

## Measured

`cargo bench -p remacs-syntax --bench html` (3 000 sections, 150 `<style>` and 150
`<script>` blocks): a typed character reparsed in 1.05 ms before, 1.75 ms with both
layers (17.2 ms while the query ran over the whole tree); 60 visible script lines
decorated in 108 µs before, 132 µs now that they are highlighted.

## The language at point (stage 2)

- `Registry::language_probes` tell the mode of injected code at an offset;
  `Editor::language_at(buffer, offset)` falls back to the buffer's mode, and
  `Editor::mode_at(offset)` gives its `Mode`. The syntax plugin answers with the layer
  whose included ranges hold the offset, while the trees match the text.
- Commenting takes the mode the first line starts in (`/* */` in `<style>`, `//` in
  `<script>`, `<!-- -->` in markup); typed pairs and expand-region take the mode at
  point. Strings and comments (`syntax_at`) and matching brackets are read from the tree
  of the language there.
- The mode line reads `HTML › CSS` while point is in injected code.
- Indentation stays text-based and mode-free, so it needs nothing.

## Open

- Stage 3: language servers of the injected languages; stage 4: formatting.
- `style="…"`, `on…="…"` and injections inside injections: #65.
