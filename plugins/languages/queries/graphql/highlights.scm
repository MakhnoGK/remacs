; GraphQL: types and their fields, operations, directives and variables.

(named_type (name) @type)
[
  (object_type_definition (name) @type)
  (interface_type_definition (name) @type)
  (enum_type_definition (name) @type)
  (input_object_type_definition (name) @type)
  (union_type_definition (name) @type)
  (scalar_type_definition (name) @type)
]
(field_definition (name) @property)
(input_value_definition (name) @property)
(field (name) @property)
(alias (name) @label)
(operation_definition (name) @function)
(fragment_name (name) @function)
(argument (name) @property)
(object_field (name) @property)
(directive "@" @attribute (name) @attribute)
(directive_definition "@" @attribute (name) @attribute)
(variable) @constant
(enum_value (name) @constant)

(string_value) @string
(description) @comment.doc
[(int_value) (float_value)] @number
(boolean_value) @boolean
(null_value) @constant.builtin
(comment) @comment

[
  "directive" "enum" "extend" "fragment" "implements" "input" "interface" "on" "repeatable"
  "scalar" "schema" "type" "union"
] @keyword
(operation_type) @keyword
[(executable_directive_location) (type_system_directive_location)] @constant.builtin
