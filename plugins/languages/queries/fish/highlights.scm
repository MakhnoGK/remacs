; Fish.

(function_definition name: (word) @function)
(command name: (word) @function.call)
((command name: (word) @function.builtin)
 (#any-of? @function.builtin
  "abbr" "argparse" "bg" "bind" "block" "breakpoint" "builtin" "cd" "command"
  "commandline" "complete" "contains" "count" "disown" "echo" "emit" "eval" "exec" "exit"
  "false" "fg" "functions" "history" "jobs" "math" "printf" "pwd" "random" "read"
  "realpath" "set" "set_color" "source" "status" "string" "test" "time" "true" "type"
  "ulimit" "wait"))

(variable_expansion) @constant
(for_statement variable: (variable_name) @constant)
(override_variable name: (variable_name) @constant)
[(home_dir_expansion) (glob)] @constant.builtin
[(integer) (float)] @number

[(double_quote_string) (single_quote_string)] @string
(escape_sequence) @string.escape
(command_substitution) @embedded

[
  "and" "begin" "case" "else" "end" "for" "function" "if" "in" "not" "or" "return"
  "switch" "while"
] @keyword
[(break) (continue)] @keyword

(comment) @comment
