; The lines of these after their first are a level deeper; `case` stands at its switch's.
[
  (block)
  (literal_value)
  (field_declaration_list)
  (interface_type)
  (argument_list)
  (parameter_list)
  (import_spec_list)
  (const_declaration)
  (var_declaration)
  (type_declaration)
  (expression_switch_statement)
  (type_switch_statement)
  (select_statement)
  (expression_case)
  (type_case)
  (default_case)
  (communication_case)
  (parenthesized_expression)
  (binary_expression)
  (selector_expression)
] @indent.begin

["}" ")" "]"] @indent.end

(expression_case "case" @indent.branch)

(type_case "case" @indent.branch)

(communication_case "case" @indent.branch)

(default_case "default" @indent.branch)

["{" "(" "["] @indent.open
