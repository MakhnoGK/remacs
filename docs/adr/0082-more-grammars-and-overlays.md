# 0082 — More grammars, and languages painted over others

Status: accepted, 2026-10-23 (issue #102, step 4)

## Decision

- **New modes**, each with its own clean-room queries (ADR 0081): Kotlin, HCL (Terraform),
  GraphQL, Protobuf, SCSS, Vue and Svelte (their scripts, styles and expressions injected
  as JavaScript or TypeScript, CSS or SCSS), and Git Commit for `COMMIT_EDITMSG` and the
  like. SCSS joins the CSS language server; the others' servers come later.
- **Overlays**: an injection pattern with `#set! injection.overlay` paints its language over
  the host's faces only where it has faces of its own, and is no language of the code for
  editing: strings and comments stay strings and comments, and it is never a document
  for servers. It brings regular expressions in JavaScript and TypeScript regex literals,
  JSDoc tags in `/** … */` comments, and Markdown's inline grammar (code spans, links) in
  every block. Markdown's fenced blocks are the language their info string names.
- **TODO in comments** is found as Emacs `hl-todo` finds it: whole words of
  `todo-keywords` (`syntax.toml`) in comment and docstring faces, rather than a comment
  grammar injected into every language's comments, whose node names differ in each.
- **Left out**: Dockerfile, gitignore and git-rebase have no grammar on crates.io built for
  the tree-sitter remacs uses (Dockerfile's needs tree-sitter 0.20 and its own copy of the
  runtime); they wait for downloadable grammars (step 6), which brought them, and Fish,
  in ADR 0087.

Measured, release build, time to the first full paint: Markdown of 3000 paragraphs
50 → 140 ms with its inline overlays, JavaScript with 1000 doc comments 16 → 27 ms.
