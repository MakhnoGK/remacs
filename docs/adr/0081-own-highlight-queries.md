# 0081 — Our own highlight queries, one capture scheme

Status: accepted, 2026-10-23 (issue #102, step 3)

Highlighting came from the queries each grammar crate ships, so their quality and capture
names varied (nvim-treesitter's old `@conditional`, `@text.title`…), and a fix meant
patching around them (Rust's all-caps constants, HTML's injections).

## Decision

- **Clean room.** Every query in `plugins/languages/queries/<language>/` is written here
  from the grammar's own node types (`node-types.json`) and the language itself; no
  editor's or grammar's queries are read or copied. The grammars stay dependencies under
  their own licenses; their bundled queries are not used, tests and benchmarks included.
- **One capture scheme**, each name drawn in a face (`highlighter.rs`); a name matches
  its dotted extensions too:
  - `comment`, `comment.doc`; `string` (`.escape`, `.regexp`, `.special`), `markup.raw`
  - `keyword`, `variable.builtin` (`self`, `this`), `markup.heading`
  - `type` (`.builtin`); `function` (`.call`), `tag`, `property` (keys of data files)
  - `function.builtin`, `function.macro`, `attribute`, `preproc`
  - `constant` (`.builtin`), `number`, `boolean`, `label`, `markup.link`
  - `diff.plus`, `diff.minus`; `embedded` for code inside strings, drawn plain;
    `string.format` for strings whose `{…}` are code; `_name` for predicates only.
  A test fails on any other capture, so a typo cannot pass as plain text.
- **Order is priority**: for one node the last pattern wins, a node inside another draws
  over it. General rules (capitalized names as types, all-caps as constants) come before
  the specific ones that override them.
- **Inheritance by concatenation**: a dialect lists its base first — C under C++ and
  Objective-C; `ecmascript` under JavaScript, JSX, TypeScript and TSX, with `jsx` and
  `typescript` on top.
- Predicates are tree-sitter's own (`#match?`, `#eq?`, `#any-of?` and their `not-`
  forms); none of our own until a query needs one.
- Each language has a sample in `each_language_highlights_its_code` with the faces its
  kinds of code must get.

Measured with the syntax benchmarks against the bundled queries: visible Rust lines
164.5 → 140.7 µs, a script in HTML 266.6 → 226.3 µs (medians), the rest unchanged.
