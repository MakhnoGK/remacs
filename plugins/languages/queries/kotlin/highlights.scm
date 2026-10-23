; Kotlin. For one node the last pattern wins; a node inside another draws over it.

((identifier) @type (#match? @type "^[A-Z]"))
((identifier) @constant (#match? @constant "^[A-Z][A-Z0-9_]+$"))
(user_type (identifier) @type)
(class_declaration name: (identifier) @type)
(object_declaration name: (identifier) @type)
(type_alias type: (identifier) @type)
(enum_entry (identifier) @constant)
(package_header (qualified_identifier (identifier) @type))

(function_declaration name: (identifier) @function)
(call_expression . (identifier) @function.call)
(call_expression . (navigation_expression (identifier) @function.call .))
(annotation "@" @attribute)
(annotation (user_type (identifier) @attribute))
(label) @label

[(string_literal) (multiline_string_literal) (character_literal)] @string
(interpolation) @embedded
(escape_sequence) @string.escape
[(number_literal) (float_literal)] @number
((identifier) @boolean (#any-of? @boolean "true" "false"))
((identifier) @constant.builtin (#eq? @constant.builtin "null"))
[(this_expression) (super_expression)] @variable.builtin
[(line_comment) (block_comment)] @comment
((block_comment) @comment.doc (#match? @comment.doc "^/\\*\\*"))
(shebang) @comment

[
  "abstract" "actual" "annotation" "as" "by" "catch" "class" "companion" "const"
  "constructor" "crossinline" "data" "do" "else" "enum" "expect" "external" "final"
  "finally" "for" "fun" "get" "if" "import" "in" "infix" "init" "inline" "inner"
  "interface" "internal" "is" "lateinit" "noinline" "object" "open" "operator" "out"
  "override" "package" "private" "protected" "public" "return" "sealed" "set" "suspend"
  "tailrec" "throw" "try" "typealias" "val" "value" "var" "vararg" "when" "where" "while"
] @keyword
