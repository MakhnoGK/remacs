; The lines of these after their first are a level deeper.
[
  (block)
  (declaration_list)
  (field_declaration_list)
  (ordered_field_declaration_list)
  (enum_variant_list)
  (field_initializer_list)
  (match_block)
  (match_arm)
  (use_list)
  (arguments)
  (parameters)
  (closure_parameters)
  (closure_expression)
  (type_arguments)
  (type_parameters)
  (where_clause)
  (array_expression)
  (tuple_expression)
  (parenthesized_expression)
  (tuple_pattern)
  (slice_pattern)
  (struct_pattern)
  (tuple_struct_pattern)
  (token_tree)
  (macro_definition)
  (field_expression)
  (binary_expression)
  (let_declaration)
  (const_item)
  (static_item)
  (assignment_expression)
  (compound_assignment_expr)
] @indent.begin

["}" ")" "]"] @indent.end

(type_arguments ">" @indent.end)

(type_parameters ">" @indent.end)

["{" "(" "["] @indent.open
