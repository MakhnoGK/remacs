; Rust. For one node the last pattern wins; a node inside another draws over it.

; Calls and definitions.
(function_item name: (identifier) @function)
(function_signature_item name: (identifier) @function)
(call_expression function: (identifier) @function.call)
(call_expression function: (scoped_identifier name: (identifier) @function.call))
(call_expression function: (field_expression field: (field_identifier) @function.call))
(generic_function function: (identifier) @function.call)
(generic_function function: (scoped_identifier name: (identifier) @function.call))
(generic_function function: (field_expression field: (field_identifier) @function.call))

; Types, and the capitalized names of variants and constructors such as `Some`.
(type_identifier) @type
(primitive_type) @type.builtin
((identifier) @type (#match? @type "^[A-Z]"))
((identifier) @constant (#match? @constant "^[A-Z][A-Z0-9_]+$"))

(macro_invocation macro: (identifier) @function.macro "!" @function.macro)
(macro_invocation
  macro: (scoped_identifier name: (identifier) @function.macro)
  "!" @function.macro)
(macro_definition name: (identifier) @function.macro)
(attribute_item) @attribute
(inner_attribute_item) @attribute

(lifetime) @label
(label) @label

(string_literal) @string
(raw_string_literal) @string
(char_literal) @string
(escape_sequence) @string.escape
(integer_literal) @number
(float_literal) @number
(boolean_literal) @boolean

(line_comment) @comment
(block_comment) @comment
(line_comment doc: (doc_comment)) @comment.doc
(block_comment doc: (doc_comment)) @comment.doc

[
  "as" "async" "await" "break" "const" "continue" "default" "dyn" "else" "enum" "extern"
  "fn" "for" "gen" "if" "impl" "in" "let" "loop" "match" "mod" "move" "pub" "raw" "ref"
  "return" "static" "struct" "trait" "try" "type" "union" "unsafe" "use" "where" "while"
  "yield" "macro_rules!"
] @keyword
(mutable_specifier) @keyword
(crate) @keyword
(super) @keyword
(self) @variable.builtin

; The string arguments of the standard formatting macros and their usual wrappers, whose
; `{…}` placeholders are code.
; shortcut: every string argument counts, not just the format string, so a literal `{x}`
; passed as a value is highlighted too; tell the format string apart if that shows up.
(macro_invocation
  macro: [(identifier) (scoped_identifier)] @_macro
  (token_tree [(string_literal) (raw_string_literal)] @string.format)
  (#match? @_macro "(^|::)(format|format_args|print|println|eprint|eprintln|write|writeln|panic|assert|assert_eq|assert_ne|debug_assert|debug_assert_eq|debug_assert_ne|unreachable|todo|unimplemented|trace|debug|info|warn|error|anyhow|bail|ensure)$"))
