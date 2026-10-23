; Make: targets as functions, variables as constants, directives as keywords.

(rule (targets) @function)
(variable_assignment name: (word) @constant)
(shell_assignment name: (word) @constant)
(define_directive name: (word) @constant)
(variable_reference) @constant
(automatic_variable) @variable.builtin
(function_call function: _ @function.builtin)
(shell_function function: _ @function.builtin)

[
  "define" "else" "endef" "endif" "export" "ifdef" "ifeq" "ifndef" "ifneq" "include"
  "-include" "sinclude" "override" "private" "undefine" "unexport" "vpath" "VPATH"
] @keyword

(string) @string
(comment) @comment
