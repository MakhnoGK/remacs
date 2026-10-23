; The lines of these after their first are a level deeper.
[
  (if_statement)
  (elif_clause)
  (else_clause)
  (do_group)
  (compound_statement)
  (subshell)
  (case_statement)
  (case_item)
  (array)
] @indent.begin

["fi" "done" "esac" "}" ")"] @indent.end

(elif_clause "elif" @indent.branch)

(else_clause "else" @indent.branch)

["then" "do" "{" "in"] @indent.open
