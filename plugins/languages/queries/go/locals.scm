; Scopes: functions, methods, function literals, blocks and what binds in a statement's
; header for its body.
[
  (function_declaration)
  (method_declaration)
  (func_literal)
  (block)
  (for_statement)
  (if_statement)
  (expression_switch_statement)
  (type_switch_statement)
] @local.scope

; Parameters, receivers and results too.
(parameter_declaration name: (identifier) @local.definition)

(variadic_parameter_declaration name: (identifier) @local.definition)

; What `:=`, `var`, `range` and a type switch bind.
(short_var_declaration left: (expression_list (identifier) @local.definition))

(var_spec name: (identifier) @local.definition)

(range_clause left: (expression_list (identifier) @local.definition))

(type_switch_statement alias: (expression_list (identifier) @local.definition))

(identifier) @local.reference

; A struct literal's field names.
(keyed_element key: (literal_element (identifier) @local.skip))
