; Rust's strings and comments, after highlights.scm: the code painted over macros'
; arguments leaves them to the file's own tree, which knows a format string by its macro.

(string_literal) @string
(raw_string_literal) @string
(char_literal) @string
(escape_sequence) @string.escape

(line_comment) @comment
(block_comment) @comment
(line_comment doc: (doc_comment)) @comment.doc
(block_comment doc: (doc_comment)) @comment.doc

; The string arguments of the standard formatting macros and their usual wrappers, whose
; `{…}` placeholders are code.
; shortcut: every string argument counts, not just the format string, so a literal `{x}`
; passed as a value is highlighted too; tell the format string apart if that shows up.
(macro_invocation
  macro: [(identifier) (scoped_identifier)] @_macro
  (token_tree [(string_literal) (raw_string_literal)] @string.format)
  (#match? @_macro "(^|::)(format|format_args|print|println|eprint|eprintln|write|writeln|panic|assert|assert_eq|assert_ne|debug_assert|debug_assert_eq|debug_assert_ne|unreachable|todo|unimplemented|trace|debug|info|warn|error|anyhow|bail|ensure)$"))
