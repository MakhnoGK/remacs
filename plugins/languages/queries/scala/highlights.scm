; Scala. For one node the last pattern wins; a node inside another draws over it.

((identifier) @type (#match? @type "^[A-Z]"))
(type_identifier) @type
(class_definition name: (identifier) @type)
(object_definition name: (identifier) @type)
(trait_definition name: (identifier) @type)
(enum_definition name: (identifier) @type)
(simple_enum_case name: (identifier) @constant)
(full_enum_case name: (identifier) @constant)
(package_identifier (identifier) @type)

(function_definition name: (identifier) @function)
(function_declaration name: (identifier) @function)
(call_expression function: (identifier) @function.call)
(call_expression function: (field_expression field: (identifier) @function.call))
(generic_function function: (identifier) @function.call)
(annotation "@" @attribute name: (_) @attribute)

[(string) (character_literal)] @string
(interpolated_string_expression) @string
(interpolation) @embedded
(escape_sequence) @string.escape
[(integer_literal) (floating_point_literal)] @number
(boolean_literal) @boolean
(null_literal) @constant.builtin
[(comment) (block_comment)] @comment
((block_comment) @comment.doc (#match? @comment.doc "^/\\*\\*"))

[
  "abstract" "as" "case" "catch" "class" "def" "derives" "do" "else" "enum" "export"
  "extends" "extension" "final" "finally" "for" "forSome" "given" "if" "implicit" "import"
  "lazy" "macro" "match" "new" "object" "override" "package" "private" "protected"
  "return" "sealed" "super" "then" "this" "throw" "trait" "try" "type" "using" "val" "var"
  "while" "with" "yield"
] @keyword
