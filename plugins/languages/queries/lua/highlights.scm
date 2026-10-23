; Lua. For one node the last pattern wins; a node inside another draws over it.

(function_declaration name: (identifier) @function)
(function_declaration name: (dot_index_expression field: (identifier) @function))
(function_declaration name: (method_index_expression method: (identifier) @function))
(function_call name: (identifier) @function.call)
(function_call name: (dot_index_expression field: (identifier) @function.call))
(function_call name: (method_index_expression method: (identifier) @function.call))
((function_call name: (identifier) @function.builtin)
 (#any-of? @function.builtin
  "assert" "error" "getmetatable" "ipairs" "load" "next" "pairs" "pcall" "print" "rawequal"
  "rawget" "rawset" "require" "select" "setmetatable" "tonumber" "tostring" "type" "xpcall"))

((identifier) @constant (#match? @constant "^[A-Z][A-Z0-9_]+$"))
((identifier) @variable.builtin (#eq? @variable.builtin "self"))
(attribute) @attribute
(label_statement) @label

(string) @string
(escape_sequence) @string.escape
(number) @number
[(true) (false)] @boolean
(nil) @constant.builtin
(comment) @comment

[
  "and" "do" "else" "elseif" "end" "for" "function" "global" "goto" "if" "in" "local" "not"
  "or" "repeat" "return" "then" "until" "while"
] @keyword
(break_statement) @keyword
