; Scopes: a function's parameters and body, a closure's, a block, and what binds in a
; condition or a loop for its body.
[
  (function_item)
  (closure_expression)
  (block)
  (match_arm)
  (for_expression)
  (if_expression)
  (while_expression)
] @local.scope

; The names patterns bind; capitalized ones are constants and enum variants.
((parameter pattern: (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

((let_declaration pattern: (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

((let_condition pattern: (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

((for_expression pattern: (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

((closure_parameters (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

((match_pattern (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

((tuple_pattern (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

((tuple_struct_pattern type: (_) (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

((slice_pattern (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

((or_pattern (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

((captured_pattern (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

((ref_pattern (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

((mut_pattern (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

((reference_pattern (identifier) @local.definition)
  (#match? @local.definition "^[a-z_]"))

(field_pattern pattern: (identifier) @local.definition)

(field_pattern name: (shorthand_field_identifier) @local.definition)

(identifier) @local.reference

; Paths' last names and functions' own are no locals.
(scoped_identifier name: (identifier) @local.skip)

(function_item name: (identifier) @local.skip)

(macro_invocation macro: (identifier) @local.skip)
