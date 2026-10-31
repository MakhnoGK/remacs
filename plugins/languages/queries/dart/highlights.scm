; Dart. For one node the last pattern wins; a node inside another draws over it.

((identifier) @type (#match? @type "^[A-Z]"))
((identifier) @constant (#match? @constant "^[A-Z][A-Z0-9_]+$"))
(type_identifier) @type
(void_type) @type.builtin
(class_declaration name: (identifier) @type)
(enum_declaration name: (identifier) @type)
(mixin_declaration name: (identifier) @type)
(enum_entry name: (identifier) @constant)

(function_signature name: (identifier) @function)
(getter_signature name: (identifier) @function)
(setter_signature name: (identifier) @function)
(constructor_signature name: (identifier) @function)
(call_expression function: (identifier) @function.call)
(call_expression function: (member_expression property: (identifier) @function.call))
(annotation "@" @attribute name: (_) @attribute)
(label (identifier) @label)

(string_literal) @string
(template_substitution) @embedded
(escape_sequence) @string.escape
(symbol_literal) @constant
[(decimal_integer_literal) (decimal_floating_point_literal) (hex_integer_literal)] @number
[(true) (false)] @boolean
(null_literal) @constant.builtin
(comment) @comment
[(documentation_block_comment) (documentation_comment)] @comment.doc
(script_tag) @comment

[
  "Function" "abstract" "as" "assert" "async" "augment" "await" "base" "break" "case"
  "catch" "class" "const" "continue" "covariant" "default" "deferred" "do" "dynamic" "else"
  "enum" "export" "extends" "extension" "external" "factory" "final" "finally" "for" "get"
  "hide" "if" "implements" "import" "in" "inline" "interface" "is" "late" "library" "mixin"
  "native" "new" "of" "on" "operator" "part" "required" "rethrow" "return" "sealed" "set"
  "show" "static" "super" "switch" "this" "throw" "try" "type" "typedef" "var" "when"
  "while" "with" "yield"
] @keyword
