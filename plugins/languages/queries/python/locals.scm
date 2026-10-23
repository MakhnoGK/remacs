; Scopes: functions, lambdas and comprehensions; a module's and a class's names are no
; locals.
[
  (function_definition)
  (lambda)
  (list_comprehension)
  (dictionary_comprehension)
  (set_comprehension)
  (generator_expression)
] @local.scope

; Parameters.
(parameters (identifier) @local.definition)

(lambda_parameters (identifier) @local.definition)

(default_parameter name: (identifier) @local.definition)

(typed_parameter (identifier) @local.definition)

(typed_default_parameter name: (identifier) @local.definition)

(list_splat_pattern (identifier) @local.definition)

(dictionary_splat_pattern (identifier) @local.definition)

; What assignments, loops and `:=` bind.
(assignment left: (identifier) @local.definition)

(pattern_list (identifier) @local.definition)

(tuple_pattern (identifier) @local.definition)

(for_statement left: (identifier) @local.definition)

(for_in_clause left: (identifier) @local.definition)

; `with … as f` and `except E as e`.
(as_pattern_target (identifier) @local.definition)

(named_expression name: (identifier) @local.definition)

(identifier) @local.reference

; Attributes and keyword arguments are named by their object and their callee.
(attribute attribute: (identifier) @local.skip)

(keyword_argument name: (identifier) @local.skip)
