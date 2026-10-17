# 0068 — Code of other languages inside a file

Status: accepted, 2026-10-17 (issue #42, stages 1–3: highlighting, language at point,
language servers)

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

## Language servers (stage 3)

- Measured first, with the real servers: the HTML server completes CSS in `<style>` but
  no JavaScript and no TypeScript in `<script>`, and checks code with its own bundled
  services, without the project's types or config.
- **Each embedded language's code is a document of its own** on the servers of its mode
  (`plugins/lsp/src/embedded.rs`), named `page.html.remacs-embedded.<ext>`: the file's
  text with every other byte a space and line ends kept, so offsets and lines are the
  buffer's and nothing is mapped. The mirror follows each edit; it is rebuilt only when
  the code's bounds move. The code comes from `Editor::embedded`, which the syntax
  plugin answers from its layers (the stage 2 language probe now derives from it).
- A question (completion, documentation, definition, code actions) goes to the servers
  of the language at its offset; without one for the embedded code, to the file's, so
  the HTML server still completes CSS without a CSS server; with none, it is answered
  with nothing at once. Answers from one
  server holding several of a buffer's documents (a page's script and its TypeScript on
  `tsc`) go to the document that was asked.
- Problems are kept to the code's ranges and listed under `server (Mode)`, apart from
  the server's problems with the file itself. The HTML server stops checking
  `<style>`/`<script>` (`validate = { styles = false, scripts = false }`), which needs
  it told it may ask for configuration: `workspace.configuration` is now advertised to
  servers the catalog configures, and to those only.
- Positions in an embedded document are read against the server's text (`text_of`);
  definitions, workspace edits and symbols name the file.

## Open

- Stage 4: formatting of the embedded block; rename and references inside it.
- `style="…"`, `on…="…"` and injections inside injections: #65.
