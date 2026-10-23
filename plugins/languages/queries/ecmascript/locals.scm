; JavaScript and TypeScript alike. Scopes: functions, methods, blocks and what binds in a
; loop or a catch for its body.
[
  (function_declaration)
  (function_expression)
  (generator_function)
  (generator_function_declaration)
  (arrow_function)
  (method_definition)
  (statement_block)
  (for_statement)
  (for_in_statement)
  (catch_clause)
] @local.scope

; Parameters and declarations, through destructuring; plain parameters are each dialect's.
(arrow_function parameter: (identifier) @local.definition)

(assignment_pattern left: (identifier) @local.definition)

(variable_declarator name: (identifier) @local.definition)

(array_pattern (identifier) @local.definition)

(object_pattern (shorthand_property_identifier_pattern) @local.definition)

(object_assignment_pattern left: (shorthand_property_identifier_pattern) @local.definition)

(pair_pattern value: (identifier) @local.definition)

(rest_pattern (identifier) @local.definition)

(catch_clause parameter: (identifier) @local.definition)

(for_in_statement left: (identifier) @local.definition)

(identifier) @local.reference

(shorthand_property_identifier) @local.reference

; A named function's own name, also in its body, is no local of it.
(function_declaration name: (identifier) @local.skip)

(function_expression name: (identifier) @local.skip)
