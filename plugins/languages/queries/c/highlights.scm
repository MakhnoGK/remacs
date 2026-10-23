; C, and the base of C++ and Objective-C. For one node the last pattern wins; a node
; inside another draws over it.

(function_declarator declarator: (identifier) @function)
(function_declarator declarator: (field_identifier) @function)
(call_expression function: (identifier) @function.call)
(call_expression function: (field_expression field: (field_identifier) @function.call))
(preproc_function_def name: (identifier) @function.macro)

(type_identifier) @type
(primitive_type) @type.builtin
(sized_type_specifier) @type.builtin
((identifier) @constant (#match? @constant "^[A-Z][A-Z0-9_]+$"))
(enumerator name: (identifier) @constant)
(preproc_def name: (identifier) @constant)
(statement_identifier) @label

[
  "#define" "#elif" "#elifdef" "#elifndef" "#else" "#endif" "#if" "#ifdef" "#ifndef"
  "#include" "defined"
] @preproc
(preproc_directive) @preproc
(system_lib_string) @string

(string_literal) @string
(char_literal) @string
(escape_sequence) @string.escape
(number_literal) @number
[(true) (false)] @boolean
(null) @constant.builtin
(comment) @comment

[
  "_Alignas" "_Alignof" "_Atomic" "_Generic" "_Noreturn" "__asm__" "__attribute__"
  "__declspec" "__extension__" "__inline" "__inline__" "__restrict__" "__thread"
  "__volatile__" "alignas" "alignof" "asm" "break" "case" "const" "constexpr"
  "continue" "default" "do" "else" "enum" "extern" "for" "goto" "if" "inline" "noreturn"
  "offsetof" "register" "restrict" "return" "sizeof" "static" "struct" "switch"
  "thread_local" "typedef" "union" "volatile" "while"
] @keyword
; `auto` among them: a type in C++, where the keyword is a node of its own.
(storage_class_specifier) @keyword
