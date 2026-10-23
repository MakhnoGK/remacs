; Scopes: methods, blocks and lambdas; a class's or a file's names are no locals.
[
  (method)
  (singleton_method)
  (block)
  (do_block)
  (lambda)
] @local.scope

; Parameters, of every kind.
(method_parameters (identifier) @local.definition)

(block_parameters (identifier) @local.definition)

(lambda_parameters (identifier) @local.definition)

(destructured_parameter (identifier) @local.definition)

(optional_parameter name: (identifier) @local.definition)

(keyword_parameter name: (identifier) @local.definition)

(splat_parameter name: (identifier) @local.definition)

(hash_splat_parameter name: (identifier) @local.definition)

(block_parameter name: (identifier) @local.definition)

; What assignments, `for` and `rescue => e` bind.
(assignment left: (identifier) @local.definition)

(left_assignment_list (identifier) @local.definition)

(operator_assignment left: (identifier) @local.definition)

(for pattern: (identifier) @local.definition)

(exception_variable (identifier) @local.definition)

(identifier) @local.reference

; A method called on an object is named by it.
(call method: (identifier) @local.skip)
