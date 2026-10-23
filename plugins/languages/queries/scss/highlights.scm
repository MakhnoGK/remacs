; SCSS, as CSS with variables, mixins, functions and control flow.

(tag_name) @tag
(nesting_selector) @tag
(universal_selector) @tag
(class_name) @type
(id_name) @type
(placeholder) @type
(pseudo_class_selector (class_name) @attribute)
(attribute_name) @attribute
(keyframes_name) @constant

(function_name) @function.call
(mixin_statement name: (identifier) @function)
(function_statement name: (identifier) @function)
(include_statement (identifier) @function.call)
(variable) @constant
(argument name: (variable) @constant)

(string_value) @string
(interpolation) @embedded
(color_value) @string
[(integer_value) (float_value)] @number
(unit) @type
(important) @keyword

(at_keyword) @keyword
[
  "@at-root" "@charset" "@debug" "@each" "@else" "@error" "@extend" "@for" "@forward"
  "@function" "@if" "@import" "@include" "@keyframes" "@media" "@mixin" "@namespace"
  "@return" "@supports" "@use" "@warn" "@while" "and" "from" "if" "in" "not" "only" "or"
  "selector" "through"
] @keyword
[(from) (to)] @keyword
(keyword_query) @constant.builtin

[(comment) (js_comment)] @comment
