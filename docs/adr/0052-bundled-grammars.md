# 0052 — Grammars built in for many languages

Status: accepted, 2026-10-09

## Decision

- **Tier one of #10** (user, 2026-10-08): remacs ships as many tree-sitter highlighters
  as it can, compiled in; language servers download on first use of a language and
  debuggers on the first run or debug (later steps).
- **25 grammars beside the initial nine**: JSON, TOML, YAML, XML, Markdown, Bash, Make,
  CMake, Diff, SQL, Python, Go, Java, C#, Swift, Objective-C, Dart, Scala, PHP, Ruby,
  Lua, Zig, Haskell, Elixir, Nix. Only crates on `tree-sitter-language` (any tree-sitter
  0.2x ABI the core loads) whose crate ships a highlights query are taken; Kotlin, HCL,
  GraphQL and Protobuf ship none, SCSS, Dockerfile and Fish still bind an old
  tree-sitter.
- **Modes by file name too**: `Mode::file_names` (`Makefile`, `CMakeLists.txt`,
  `.zshrc`, `Gemfile`…) win over extensions.
- **Older capture names get faces**: grammars still on nvim-treesitter's former names
  (`@conditional`, `@repeat`, `@include`, `@boolean`, `@method`, `@text.title`,
  `@text.literal`…) map to the same faces as their current names.
- Markdown highlights block structure only: inline markup is a second grammar injected
  into each block, and remacs has no injections yet.

## Consequences

- The release binary grows from about 22 MB to 55 MB, mostly the parse tables of
  Objective-C, C#, Swift, Scala and Haskell (4–5 MB each). They are read-only pages,
  mapped in only when a buffer of that language is parsed, so memory does not grow.
- Release builds compile 25 more C parsers.

## Open

- Injections: Markdown inline markup and fenced code, `<script>`/`<style>` in HTML.
- Extensions and file names per mode in config.
