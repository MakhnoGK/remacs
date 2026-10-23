; The lines of these after their first are a level deeper.
[
  (block)
  (class_body)
  (interface_body)
  (enum_body)
  (record_declaration)
  (annotation_type_body)
  (constructor_body)
  (switch_block)
  (switch_block_statement_group)
  (switch_rule)
  (array_initializer)
  (element_value_array_initializer)
  (argument_list)
  (formal_parameters)
  (annotation_argument_list)
  (parenthesized_expression)
  (if_statement)
  (for_statement)
  (enhanced_for_statement)
  (while_statement)
  (do_statement)
  (lambda_expression)
  (method_invocation)
  (binary_expression)
  (ternary_expression)
  (variable_declarator)
  (assignment_expression)
  (return_statement)
] @indent.begin

["}" ")" "]"] @indent.end

(if_statement "else" @indent.branch)

(do_statement "while" @indent.branch)

["{" "(" "["] @indent.open
