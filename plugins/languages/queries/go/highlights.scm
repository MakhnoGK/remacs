; Go. For one node the last pattern wins; a node inside another draws over it.

(function_declaration name: (identifier) @function)
(method_declaration name: (field_identifier) @function)
(method_elem name: (field_identifier) @function)
(call_expression function: (identifier) @function.call)
(call_expression function: (selector_expression field: (field_identifier) @function.call))
((call_expression function: (identifier) @function.builtin)
 (#any-of? @function.builtin
  "append" "cap" "clear" "close" "complex" "copy" "delete" "imag" "len" "make" "max" "min"
  "new" "panic" "print" "println" "real" "recover"))

(type_identifier) @type
((type_identifier) @type.builtin
 (#any-of? @type.builtin
  "any" "bool" "byte" "comparable" "complex64" "complex128" "error" "float32" "float64"
  "int" "int8" "int16" "int32" "int64" "rune" "string" "uint" "uint8" "uint16" "uint32"
  "uint64" "uintptr"))
(package_identifier) @type
(const_spec name: (identifier) @constant)
(iota) @constant.builtin
(label_name) @label

[(interpreted_string_literal) (raw_string_literal) (rune_literal)] @string
(escape_sequence) @string.escape
(field_declaration tag: (_) @attribute)
[(int_literal) (float_literal) (imaginary_literal)] @number
[(true) (false)] @boolean
(nil) @constant.builtin
(comment) @comment

[
  "break" "case" "chan" "const" "continue" "default" "defer" "else" "fallthrough" "for"
  "func" "go" "goto" "if" "import" "interface" "map" "package" "range" "return" "select"
  "struct" "switch" "type" "var"
] @keyword
