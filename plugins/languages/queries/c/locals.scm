; Scopes: a function's parameters and body, a block, a loop's declarations.
[
  (function_definition)
  (compound_statement)
  (for_statement)
] @local.scope

; The names declarators declare, through pointers and arrays; at file level there is no
; scope, so globals are none.
(parameter_declaration declarator: (identifier) @local.definition)

(init_declarator declarator: (identifier) @local.definition)

(declaration declarator: (identifier) @local.definition)

(pointer_declarator declarator: (identifier) @local.definition)

(array_declarator declarator: (identifier) @local.definition)

(identifier) @local.reference
