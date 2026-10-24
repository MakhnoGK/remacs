# 0088 — Semantic tokens over tree-sitter

Status: accepted, 2026-10-24 (issue #102, step 10)

Tree-sitter knows a file's shape, not its meaning: a constant named in lowercase, a call
through a variable, a type it can tell only by its capital letter. Language servers know,
and say so in semantic tokens.

## Decision

- **Whole files, as each version changes**: a document asks its server for
  `textDocument/semanticTokens/full` once per version it sends, like colors (ADR 0067),
  again when the server asks (`workspace/semanticTokens/refresh`, rust-analyzer once it
  loaded the project) and when the painting changes. Tokens an edit touches are dropped
  until the answer comes, those after it move with it. Requests superseded in a batch
  are not sent.
- **Painted over tree-sitter**: the LSP plugin's decorator comes after the syntax
  plugin's, so its faces draw over theirs; a token without a face leaves tree-sitter's.
  The client says `augmentsSyntaxTokens`, so servers may leave out what tree-sitter
  paints anyway.
- **In the faces of remacs's capture scheme** (ADR 0081): each token type takes a capture
  name, `type.modifier` for a modifier that changes it (`function.declaration` is a
  definition, `function` a call), and gets that capture's face and level. So
  `syntax.toml`'s `highlight-level` holds for them too (ADR 0081, step 9), and
  `local-variables` for parameters and variables. The scheme gains `variable`
  (`Face::Variable`, level 4).
- **Only what tree-sitter cannot tell**: comment, string, number, keyword, operator and
  the like take no capture. Told `augmentsSyntaxTokens`, rust-analyzer leaves out
  comments, strings and keywords but still sends a `comment` over each whole doc comment
  (measured, 2026-10-24); painted, it would hide the TODOs the syntax plugin marks in it,
  and servers that ignore the flag send them all.
- **Data, as everything tunable**: the built-in table (`semantic::CAPTURES`: the LSP's
  types, then rust-analyzer's and TypeScript's own) is listed in `lsp.toml`, whose
  `[semantic-token-captures]` changes any entry, `""` painting nothing;
  `semantic-tokens = false` turns them off. A test keeps the list and the table alike.
- Decoding happens on the reader thread, line by line, each token's column counted on
  from the one before it on its line.

## Measured

`cargo bench -p remacs-lsp --bench semantic_tokens`, decoding 100 000 tokens of a megabyte,
medians, each token converted with `from_lsp` against counting on from the one before:

| File | Each from its line's start | Counted on |
|---|---|---|
| 20 000 lines | 527 ms | 56 ms |
| one line (minified) | 34 s | 8.1 ms |

Picking each token's face on the editor's thread takes 3 ms for them all; the decorator
looks only at those shown.

## Later

- `semanticTokens/full/delta` and `/range`, should whole answers of big files cost.
- Tokens across lines (`multilineTokenSupport`) and overlapping ones are not asked for.
