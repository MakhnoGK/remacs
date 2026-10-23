; The lines of these after their first are a level deeper.
[
  (function_definition)
  (compound_statement)
  (field_declaration_list)
  (enumerator_list)
  (initializer_list)
  (argument_list)
  (parameter_list)
  (parenthesized_expression)
  (if_statement)
  (for_statement)
  (while_statement)
  (do_statement)
  (case_statement)
  (init_declarator)
  (binary_expression)
  (conditional_expression)
  (assignment_expression)
  (return_statement)
] @indent.begin

["}" ")" "]"] @indent.end

; A function's `{` on a line of its own stands at its level.
(function_definition
  body: (compound_statement "{" @indent.branch))

(else_clause "else" @indent.branch)

(do_statement "while" @indent.branch)

["{" "(" "["] @indent.open
