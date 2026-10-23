; HCL, as Terraform writes it: block types as keywords, their labels as types.

(block . (identifier) @keyword)
(attribute (identifier) @property)
(object_elem key: (expression (variable_expr (identifier) @property)))
(function_call (identifier) @function.call)
(get_attr (identifier) @property)
((variable_expr (identifier) @variable.builtin)
 (#any-of? @variable.builtin "var" "local" "module" "data" "path" "terraform" "each" "count" "self"))

[(string_lit) (quoted_template) (heredoc_template)] @string
(block (string_lit) @type)
(template_interpolation) @embedded
[(heredoc_start) (heredoc_identifier)] @string
(numeric_lit) @number
(bool_lit) @boolean
(null_lit) @constant.builtin
(comment) @comment

["for" "in" "if" "else" "endif" "endfor"] @keyword
