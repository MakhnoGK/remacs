; The lines of these after their first are a level deeper; `when`, `else` and `rescue`
; stand at the level of what they continue.
[
  (method)
  (singleton_method)
  (class)
  (singleton_class)
  (module)
  (if)
  (unless)
  (elsif)
  (else)
  (while)
  (until)
  (for)
  (case)
  (when)
  (begin)
  (rescue)
  (ensure)
  (do_block)
  (block)
  (lambda)
  (hash)
  (array)
  (argument_list)
  (method_parameters)
  (block_parameters)
  (parenthesized_statements)
] @indent.begin

["end" "}" ")" "]"] @indent.end

(elsif "elsif" @indent.branch)

(else "else" @indent.branch)

(when "when" @indent.branch)

(rescue "rescue" @indent.branch)

(ensure "ensure" @indent.branch)

["{" "(" "[" "|" "do"] @indent.open
