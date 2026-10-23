; Rust's code, also in macros' arguments; its strings and comments are text.scm's. For one
; node the last pattern wins; a node inside another draws over it.

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

(integer_literal) @number
(float_literal) @number
(boolean_literal) @boolean

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
