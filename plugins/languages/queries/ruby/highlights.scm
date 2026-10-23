; Ruby. For one node the last pattern wins; a node inside another draws over it.

(constant) @type
((constant) @constant (#match? @constant "^[A-Z][A-Z0-9_]+$"))
(method name: (_) @function)
(singleton_method name: (_) @function)
(call method: (identifier) @function.call)
((call method: (identifier) @function.builtin)
 (#any-of? @function.builtin
  "attr_accessor" "attr_reader" "attr_writer" "extend" "include" "loop" "module_function"
  "p" "prepend" "private" "protected" "public" "puts" "raise" "require" "require_relative"))
; Without arguments these are no calls, but no one names a variable so.
((identifier) @function.builtin
 (#any-of? @function.builtin "private" "protected" "public" "module_function"))

[(instance_variable) (class_variable) (global_variable)] @constant
[(self) (super)] @variable.builtin

[(string) (bare_string) (character) (chained_string)] @string
[(simple_symbol) (delimited_symbol) (hash_key_symbol) (bare_symbol)] @constant
(interpolation) @embedded
(escape_sequence) @string.escape
(regex) @string.regexp
[(integer) (float)] @number
[(true) (false)] @boolean
(nil) @constant.builtin
(comment) @comment

[
  "BEGIN" "END" "alias" "and" "begin" "break" "case" "class" "def" "do" "else" "elsif"
  "end" "ensure" "for" "if" "in" "module" "next" "not" "or" "redo" "rescue" "retry"
  "return" "then" "undef" "unless" "until" "when" "while" "yield"
] @keyword
