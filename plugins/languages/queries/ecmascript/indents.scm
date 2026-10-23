; JavaScript and TypeScript alike. The lines of these after their first are a level deeper.
[
  (statement_block)
  (class_body)
  (object)
  (array)
  (object_pattern)
  (array_pattern)
  (arguments)
  (formal_parameters)
  (named_imports)
  (export_clause)
  (switch_body)
  (switch_case)
  (switch_default)
  (parenthesized_expression)
  (template_substitution)
  (if_statement)
  (for_statement)
  (for_in_statement)
  (while_statement)
  (do_statement)
  (member_expression)
  (binary_expression)
  (ternary_expression)
  (variable_declarator)
  (assignment_expression)
  (arrow_function)
  (return_statement)
  (pair)
] @indent.begin

["}" ")" "]"] @indent.end

(else_clause "else" @indent.branch)

(do_statement "while" @indent.branch)

["{" "(" "["] @indent.open
