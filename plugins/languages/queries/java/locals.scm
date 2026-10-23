; Scopes: methods, constructors, lambdas, blocks and what binds in a statement's header
; for its body; a class's fields are no locals.
[
  (method_declaration)
  (constructor_declaration)
  (lambda_expression)
  (block)
  (for_statement)
  (enhanced_for_statement)
  (catch_clause)
  (try_with_resources_statement)
] @local.scope

(formal_parameter name: (identifier) @local.definition)

(catch_formal_parameter name: (identifier) @local.definition)

(variable_declarator name: (identifier) @local.definition)

(enhanced_for_statement name: (identifier) @local.definition)

(lambda_expression parameters: (identifier) @local.definition)

(inferred_parameters (identifier) @local.definition)

(resource name: (identifier) @local.definition)

(instanceof_expression name: (identifier) @local.definition)

(type_pattern (identifier) @local.definition)

(identifier) @local.reference

; Fields and methods are named by their object.
(field_access field: (identifier) @local.skip)

(method_invocation name: (identifier) @local.skip)

(method_declaration name: (identifier) @local.skip)
