; Extends C's.
[
  (lambda_expression)
  (for_range_loop)
  (catch_clause)
] @local.scope

(optional_parameter_declaration declarator: (identifier) @local.definition)

(reference_declarator (identifier) @local.definition)

(for_range_loop declarator: (identifier) @local.definition)

(structured_binding_declarator (identifier) @local.definition)
