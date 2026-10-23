; CMake: commands as calls, the control commands as keywords, `${…}` as variables.

(normal_command (identifier) @function.call)
[
  (if) (elseif) (else) (endif) (foreach) (endforeach) (while) (endwhile)
  (function) (endfunction) (macro) (endmacro) (block) (endblock)
] @keyword
(function_command (argument_list . (argument) @function))
(macro_command (argument_list . (argument) @function))

(variable_ref) @constant
["CACHE" "ENV"] @keyword
((unquoted_argument) @constant (#match? @constant "^[A-Z][A-Z0-9_]+$"))

[(quoted_argument) (bracket_argument)] @string
(escape_sequence) @string.escape
[(line_comment) (bracket_comment)] @comment
