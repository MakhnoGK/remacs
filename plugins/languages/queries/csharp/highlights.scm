; C#. For one node the last pattern wins; a node inside another draws over it.

; Methods and properties are capitalized too, so types go by where they stand.
(variable_declaration type: (identifier) @type)
(parameter type: (identifier) @type)
(property_declaration type: (identifier) @type)
(method_declaration returns: (identifier) @type)
(object_creation_expression type: (identifier) @type)
(base_list (identifier) @type)
(type_argument_list (identifier) @type)
(generic_name (identifier) @type)
(qualified_name name: (identifier) @type)
(member_access_expression
  expression: (identifier) @type (#match? @type "^[A-Z]"))
(predefined_type) @type.builtin
(implicit_type) @type.builtin

(method_declaration name: (identifier) @function)
(local_function_statement name: (identifier) @function)
(constructor_declaration name: (identifier) @function)
(destructor_declaration name: (identifier) @function)
(invocation_expression function: (identifier) @function.call)
(invocation_expression
  function: (member_access_expression name: (identifier) @function.call))
(invocation_expression function: (generic_name (identifier) @function.call))
[
  (class_declaration name: (identifier) @type)
  (struct_declaration name: (identifier) @type)
  (record_declaration name: (identifier) @type)
  (interface_declaration name: (identifier) @type)
  (enum_declaration name: (identifier) @type)
  (delegate_declaration name: (identifier) @type)
]
(enum_member_declaration name: (identifier) @constant)
(namespace_declaration name: (_) @type)
(file_scoped_namespace_declaration name: (_) @type)
(attribute name: (_) @attribute)
(labeled_statement (identifier) @label)

[(string_literal) (verbatim_string_literal) (raw_string_literal) (character_literal)] @string
(interpolated_string_expression) @string
(interpolation) @embedded
(escape_sequence) @string.escape
[(integer_literal) (real_literal)] @number
(boolean_literal) @boolean
(null_literal) @constant.builtin
(comment) @comment
((comment) @comment.doc (#match? @comment.doc "^///"))
[
  "#define" "#elif" "#else" "#endif" "#endregion" "#error" "#if" "#line" "#nullable"
  "#pragma" "#region" "#undef" "#warning"
] @preproc

[
  "abstract" "add" "alias" "as" "ascending" "async" "await" "base" "break" "by" "case"
  "catch" "checked" "class" "const" "continue" "default" "delegate" "descending" "do"
  "else" "enum" "equals" "event" "explicit" "extern" "file" "finally" "fixed" "for"
  "foreach" "from" "get" "global" "goto" "group" "if" "implicit" "in" "init" "interface"
  "internal" "into" "is" "join" "let" "lock" "namespace" "new" "not" "on" "operator" "or"
  "orderby" "out" "override" "params" "partial" "private" "protected" "public" "readonly"
  "record" "ref" "remove" "required" "return" "scoped" "sealed" "select" "set" "sizeof"
  "stackalloc" "static" "struct" "switch" "this" "throw" "try" "typeof" "unchecked"
  "unsafe" "using" "virtual" "volatile" "when" "where" "while" "with" "yield"
] @keyword
