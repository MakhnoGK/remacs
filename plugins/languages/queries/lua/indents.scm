; The lines of these after their first are a level deeper.
[
  (function_declaration)
  (function_definition)
  (if_statement)
  (elseif_statement)
  (else_statement)
  (for_statement)
  (while_statement)
  (repeat_statement)
  (do_statement)
  (table_constructor)
  (arguments)
  (parameters)
  (parenthesized_expression)
] @indent.begin

["end" "until" "}" ")" "]"] @indent.end

(elseif_statement "elseif" @indent.branch)

(else_statement "else" @indent.branch)

["{" "(" "[" "then" "do"] @indent.open
