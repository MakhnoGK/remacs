; Java. For one node the last pattern wins; a node inside another draws over it.

(method_declaration name: (identifier) @function)
(constructor_declaration name: (identifier) @function)
(compact_constructor_declaration name: (identifier) @function)
(method_invocation name: (identifier) @function.call)
(method_reference (identifier) @function.call)

((identifier) @type (#match? @type "^[A-Z]"))
((identifier) @constant (#match? @constant "^[A-Z][A-Z0-9_]+$"))
(type_identifier) @type
[(integral_type) (floating_point_type) (boolean_type) (void_type)] @type.builtin
(enum_constant name: (identifier) @constant)
(annotation "@" @attribute name: (_) @attribute)
(marker_annotation "@" @attribute name: (_) @attribute)
(labeled_statement (identifier) @label)

[(string_literal) (character_literal)] @string
(string_interpolation) @embedded
(escape_sequence) @string.escape
[
  (decimal_integer_literal) (hex_integer_literal) (octal_integer_literal)
  (binary_integer_literal) (decimal_floating_point_literal) (hex_floating_point_literal)
] @number
[(true) (false)] @boolean
(null_literal) @constant.builtin
[(this) (super)] @variable.builtin
(line_comment) @comment
(block_comment) @comment
((block_comment) @comment.doc (#match? @comment.doc "^/\\*\\*"))

[
  "@interface" "abstract" "assert" "break" "case" "catch" "class" "continue" "default" "do"
  "else" "enum" "exports" "extends" "final" "finally" "for" "if" "implements" "import"
  "instanceof" "interface" "module" "native" "new" "non-sealed" "open" "opens" "package"
  "permits" "private" "protected" "provides" "public" "record" "requires" "return" "sealed"
  "static" "strictfp" "switch" "synchronized" "throw" "throws" "to" "transient"
  "transitive" "try" "uses" "volatile" "when" "while" "with" "yield"
] @keyword
