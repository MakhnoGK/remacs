; PHP, with the HTML around `<?php` tags left as text.

((name) @type (#match? @type "^[A-Z]"))
((name) @constant (#match? @constant "^[A-Z][A-Z0-9_]+$"))
(named_type (name) @type)
(qualified_name (name) @type)
(namespace_name (name) @type)
[(primitive_type) (cast_type) (bottom_type)] @type.builtin
(class_declaration name: (name) @type)
(interface_declaration name: (name) @type)
(trait_declaration name: (name) @type)
(enum_declaration name: (name) @type)
(enum_case name: (name) @constant)
(const_element (name) @constant)

(function_definition name: (name) @function)
(method_declaration name: (name) @function)
(function_call_expression function: [(name) (qualified_name)] @function.call)
(member_call_expression name: (name) @function.call)
(nullsafe_member_call_expression name: (name) @function.call)
(scoped_call_expression name: (name) @function.call)
(attribute (name) @attribute)
(named_label_statement (name) @label)

(variable_name) @constant
((variable_name (name) @variable.builtin) (#eq? @variable.builtin "this"))
[(php_tag) (php_end_tag)] @preproc

[(string) (encapsed_string) (heredoc) (nowdoc)] @string
(escape_sequence) @string.escape
[(integer) (float)] @number
(boolean) @boolean
(null) @constant.builtin
(comment) @comment
((comment) @comment.doc (#match? @comment.doc "^/\\*\\*"))

[
  "abstract" "and" "as" "break" "case" "catch" "class" "clone" "const" "continue" "declare"
  "default" "do" "echo" "else" "elseif" "enddeclare" "endfor" "endforeach" "endif"
  "endswitch" "endwhile" "enum" "exit" "extends" "final" "finally" "fn" "for" "foreach"
  "function" "global" "goto" "if" "implements" "include" "include_once" "instanceof"
  "insteadof" "interface" "list" "match" "namespace" "new" "or" "parent" "print" "private"
  "protected" "public" "readonly" "require" "require_once" "return" "self" "static" "switch"
  "throw" "trait" "try" "unset" "use" "while" "xor" "yield"
] @keyword
