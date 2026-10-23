; Bash and the shells like it.

(function_definition name: (word) @function)
(command_name (word) @function.call)
((command_name (word) @function.builtin)
 (#any-of? @function.builtin
  "alias" "bg" "bind" "builtin" "cd" "command" "compgen" "complete" "echo" "enable" "eval"
  "exec" "exit" "fg" "getopts" "hash" "jobs" "kill" "let" "printf" "pushd" "popd" "pwd"
  "read" "return" "set" "shift" "shopt" "source" "test" "trap" "type" "ulimit" "umask"
  "unalias" "wait" "." ":" "true" "false"))

(variable_assignment name: (variable_name) @constant)
(simple_expansion) @constant
(expansion) @constant
(special_variable_name) @variable.builtin
(file_descriptor) @number
(number) @number
(test_operator) @keyword

[(string) (raw_string) (ansi_c_string) (translated_string)] @string
[(heredoc_body) (heredoc_start) (heredoc_end)] @string
(command_substitution) @embedded
(regex) @string.regexp

[
  "case" "declare" "do" "done" "elif" "else" "esac" "export" "fi" "for" "function" "if"
  "in" "local" "readonly" "select" "then" "typeset" "unset" "unsetenv" "until" "while"
] @keyword

(comment) @comment
