; C++, after C's patterns.

(function_declarator declarator: (qualified_identifier name: (identifier) @function))
(function_declarator declarator: (destructor_name) @function)
(function_declarator declarator: (operator_name) @function)
(call_expression function: (qualified_identifier name: (identifier) @function.call))
(call_expression function: (template_function name: (identifier) @function.call))
(template_method name: (field_identifier) @function.call)

(namespace_identifier) @type
(auto) @type.builtin
(concept_definition name: (identifier) @type)

(raw_string_literal) @string
(literal_suffix) @type
(this) @variable.builtin

[
  "catch" "class" "co_await" "co_return" "co_yield" "concept" "consteval" "constinit"
  "decltype" "delete" "explicit" "final" "friend" "mutable" "namespace" "new" "noexcept"
  "operator" "override" "private" "protected" "public" "requires" "static_assert"
  "template" "throw" "try" "typename" "using" "virtual"
  "and" "and_eq" "bitand" "bitor" "compl" "not" "not_eq" "or" "or_eq" "xor" "xor_eq"
] @keyword
(access_specifier) @keyword
