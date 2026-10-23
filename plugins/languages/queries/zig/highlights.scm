; Zig. For one node the last pattern wins; a node inside another draws over it.

((identifier) @type (#match? @type "^[A-Z]"))
((identifier) @constant (#match? @constant "^[A-Z][A-Z0-9_]+$"))
(builtin_type) @type.builtin
(function_declaration name: (identifier) @function)
(function_signature name: (identifier) @function)
(call_expression function: (identifier) @function.call)
(call_expression function: (field_expression member: (identifier) @function.call))
(builtin_function (builtin_identifier) @function.builtin)
[(block_label) (break_label)] @label

[(string) (multiline_string) (character)] @string
(escape_sequence) @string.escape
[(integer) (float)] @number
(boolean) @boolean
["null" "undefined"] @constant.builtin
(comment) @comment
((comment) @comment.doc (#match? @comment.doc "^//[/!]"))

[
  "addrspace" "align" "allowzero" "and" "anyframe" "asm" "async" "await" "break" "callconv"
  "catch" "comptime" "const" "continue" "defer" "else" "enum" "errdefer" "error" "export"
  "extern" "fn" "for" "if" "inline" "linksection" "noalias" "noinline" "nosuspend" "opaque"
  "or" "orelse" "packed" "pub" "resume" "return" "struct" "suspend" "switch" "test"
  "threadlocal" "try" "union" "unreachable" "usingnamespace" "var" "volatile" "while"
] @keyword
