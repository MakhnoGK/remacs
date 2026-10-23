; Swift. For one node the last pattern wins; a node inside another draws over it.

(function_declaration name: (simple_identifier) @function)
(protocol_function_declaration name: (simple_identifier) @function)
(call_expression (simple_identifier) @function.call)
(call_expression (navigation_expression suffix: (navigation_suffix suffix: (simple_identifier) @function.call)))
(macro_invocation (simple_identifier) @function.macro)

(type_identifier) @type
((simple_identifier) @type (#match? @type "^[A-Z]"))
(enum_entry name: (simple_identifier) @constant)
(attribute) @attribute
(attribute (user_type (type_identifier) @attribute))
(statement_label) @label

[(line_string_literal) (multi_line_string_literal) (raw_string_literal)] @string
(interpolated_expression) @embedded
(str_escaped_char) @string.escape
(regex_literal) @string.regexp
[(integer_literal) (real_literal) (hex_literal) (oct_literal) (bin_literal)] @number
(boolean_literal) @boolean
(nil_literal) @constant.builtin
(special_literal) @constant.builtin
(self_expression) @variable.builtin
(super_expression) @variable.builtin
[(comment) (multiline_comment)] @comment
((comment) @comment.doc (#match? @comment.doc "^///"))
(directive) @preproc

[
  "actor" "any" "as" "associatedtype" "async" "await" "break" "case" "class" "continue"
  "convenience" "deinit" "do" "dynamic" "enum" "extension" "fallthrough"
  "fileprivate" "final" "for" "func" "get" "guard" "if" "import" "in" "indirect" "infix"
  "init" "inout" "internal" "is" "lazy" "let" "macro" "mutating" "nonisolated"
  "nonmutating" "open" "operator" "optional" "override" "package" "postfix"
  "precedencegroup" "prefix" "private" "protocol" "public" "repeat" "required" "return"
  "set" "some" "static" "struct" "subscript" "switch" "typealias" "unowned" "var" "weak"
  "while" "willSet" "didSet" "yield"
] @keyword
(else) @keyword
(throws) @keyword
[(throw_keyword) (catch_keyword) (try_operator) (where_keyword) (default_keyword)] @keyword
(function_modifier) @keyword
