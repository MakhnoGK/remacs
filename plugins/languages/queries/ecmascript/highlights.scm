; JavaScript and TypeScript alike, the base of their dialects. For one node the last
; pattern wins; a node inside another draws over it.

(function_declaration name: (identifier) @function)
(function_expression name: (identifier) @function)
(generator_function_declaration name: (identifier) @function)
(method_definition name: [(property_identifier) (private_property_identifier)] @function)
(variable_declarator
  name: (identifier) @function
  value: [(arrow_function) (function_expression)])
(pair
  key: (property_identifier) @function
  value: [(arrow_function) (function_expression)])
(call_expression function: (identifier) @function.call)
(call_expression
  function: (member_expression
    property: [(property_identifier) (private_property_identifier)] @function.call))

; Capitalized names are classes and constructors; all-caps ones constants.
((identifier) @type (#match? @type "^[A-Z]"))
((identifier) @constant (#match? @constant "^[A-Z][A-Z0-9_]+$"))
(class_declaration name: (_) @type)
(class name: (_) @type)
(new_expression constructor: (identifier) @type)

(decorator "@" @attribute (identifier) @attribute)
(decorator "@" @attribute (call_expression function: (identifier) @attribute))
(statement_identifier) @label

(string) @string
(template_string) @string
(template_substitution) @embedded
(escape_sequence) @string.escape
(regex) @string.regexp
(number) @number
[(true) (false)] @boolean
[(null) (undefined)] @constant.builtin
[(this) (super)] @variable.builtin
(comment) @comment
((comment) @comment.doc (#match? @comment.doc "^/\\*\\*"))
(hash_bang_line) @comment

[
  "as" "async" "await" "break" "case" "catch" "class" "const" "continue" "debugger"
  "default" "delete" "do" "else" "export" "extends" "finally" "for" "from" "function"
  "get" "if" "import" "in" "instanceof" "let" "new" "of" "return" "set" "static" "switch"
  "throw" "try" "typeof" "var" "void" "while" "with" "yield"
] @keyword
