; The lines of these after their first are a level deeper.
[
  (function_definition)
  (class_definition)
  (if_statement)
  (elif_clause)
  (else_clause)
  (for_statement)
  (while_statement)
  (try_statement)
  (except_clause)
  (finally_clause)
  (with_statement)
  (match_statement)
  (case_clause)
  (argument_list)
  (parameters)
  (list)
  (dictionary)
  (set)
  (tuple)
  (parenthesized_expression)
  (list_comprehension)
  (dictionary_comprehension)
  (set_comprehension)
  (generator_expression)
  (subscript)
] @indent.begin

["}" ")" "]"] @indent.end

(elif_clause "elif" @indent.branch)

(else_clause "else" @indent.branch)

(except_clause "except" @indent.branch)

(finally_clause "finally" @indent.branch)

[":" "{" "(" "["] @indent.open
