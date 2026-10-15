# 0005 — Syntax highlighting

Status: accepted, 2026-10-04 (M4, step 1)

## Decision

- **tree-sitter 0.27**, one grammar per major mode; `languages` lists the grammars and
  their highlight queries, `syntax` (`SyntaxPlugin::new(GRAMMARS)`) is language-agnostic.
- **One parser thread** for all buffers. It reads straight from a `Snapshot`'s pieces
  (no copy), keeps only the newest request per buffer and notifies the UI when done.
- **Edits without an edit log**: `Snapshot::edit_from(old)` finds the single changed range
  by comparing chunks, shared ones by address, so its cost follows pieces, not bytes. It
  works the same for typing, undo and anything else. The UI-side tree is edited at once
  (`Tree::edit`), so highlights never drift while the incremental reparse runs.
- **Highlighting is on demand**, like jit-lock: a frame asks each `Decorator` once for the
  visible lines' bytes and cuts the faces to each line (`Editor::decorations_by_line`);
  lines cut short of long middles are asked for one at a time. No highlight cache.
- Capture precedence follows tree-sitter-highlight: for one node the last pattern wins, a
  nested node draws over its parent, a capture without a face keeps the parent's.
- Buffers over 16 MiB are not parsed.

## Core additions

- `Buffer::local`/`set_local`: per-buffer plugin state, dropped with the buffer.
- `Registry::post_command_hooks` (also run on `Editor::refresh`) and `Registry::decorators`.
- `Face` (theme-agnostic); the app maps faces to Modus colors and layers faces, region and
  cursor into one highlight list.

## Open

- Injections (`<script>`/`<style>` in HTML, JS template strings) are not highlighted.
- `#is-not? local` predicates are ignored (no locals tracking).
