# 0086 — Local variables highlighted

Status: accepted, 2026-10-24 (issue #102, step 5c)

## Decision

- **Locals queries of our own** (clean room, as ADR 0081), `queries/<lang>/locals.scm`,
  for Rust, C, C++, JavaScript/JSX, TypeScript/TSX, Python, Go, Java, Ruby and Lua.
  Captures: `@local.scope` (a function, closure, block, or what binds in a statement's
  header for its body), `@local.definition` (a parameter, a `let`/`:=`/`local`, a loop's or
  a pattern's names), `@local.reference` (any name), `@local.skip` (names that are never a
  local's, like a field after a dot or a keyword argument).
- **The rule**: a name is a local's where a scope around it defines it, anywhere in the
  scope (a comprehension uses its names before its `for`). A file's, a module's or a
  class's names are no locals, having no scope; Rust's capitalized names in patterns are
  constants and variants.
- **A new face, `Face::Variable`**, the theme's `variable` color (Modus's cyan), drawn only
  where nothing else is: a local closure called keeps the call's face. `syntax.toml`
  `local-variables` turns it off.
- **Read per frame, for the lines drawn**: one query over them finds the scopes around
  them and their names; the definitions of those scopes are read once with the query's
  scope and definition patterns only, and kept while the tree and the scopes stay. Nodes
  are placed by their bytes, not walked up through their parents, which tree-sitter finds
  from the root each time. Measured (`benches/decorations.rs`, 60 lines of a Rust
  function): 145 µs a frame without locals, 257 µs with them, 605 µs when the
  definitions are read anew, as after an edit; the first version, walking parents and
  reading whole scopes every frame, took 1.15 ms.
- **A scope as large as a file**, a bundled script wrapped in one function or a spec in
  one `describe`, is read only within 16 KiB steps around the lines drawn, which stay put
  while they scroll: a key typed in a function of 1 MB costs 2.5 ms instead of 73 ms. A
  name defined farther off is not colored there. The names of 8 scopes are kept, so the
  lines drawn on each side of a fold, or in other windows, are not read anew each frame.

## Open

- Code of other languages in a file (scripts in HTML and Vue, fenced Markdown) gets its
  language's locals, from its own tree; an overlay's (ADR 0081), like a Rust macro's
  arguments, are the host's.
- Names are matched, not resolved: a local shadowing a global colors the global's uses
  in the scope too.
