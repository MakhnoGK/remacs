; Scopes: functions, blocks and loops; a file's `local`s are the module's, no locals.
[
  (function_declaration)
  (function_definition)
  (do_statement)
  (for_statement)
  (while_statement)
  (repeat_statement)
  (if_statement)
] @local.scope

(parameters name: (identifier) @local.definition)

; Only `local` declares one: a plain assignment sets a global.
(variable_declaration (variable_list name: (identifier) @local.definition))

(variable_declaration
  (assignment_statement (variable_list name: (identifier) @local.definition)))

(for_generic_clause (variable_list name: (identifier) @local.definition))

(for_numeric_clause name: (identifier) @local.definition)

(identifier) @local.reference

; Fields and methods are named by their table.
(dot_index_expression field: (identifier) @local.skip)

(method_index_expression method: (identifier) @local.skip)

(field name: (identifier) @local.skip)
