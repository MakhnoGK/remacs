; CSS. For one node the last pattern wins; a node inside another draws over it.

(tag_name) @tag
(nesting_selector) @tag
(universal_selector) @tag
(class_name) @type
(id_name) @type
(namespace_name) @type
(pseudo_class_selector (class_name) @attribute)
(pseudo_element_selector (tag_name) @attribute)
(attribute_name) @attribute
(keyframes_name) @constant

(function_name) @function.call
(string_value) @string
(escape_sequence) @string.escape
(color_value) @string
(integer_value) @number
(float_value) @number
(unit) @type
(important) @keyword

(at_keyword) @keyword
[
  "@charset" "@import" "@keyframes" "@media" "@namespace" "@scope" "@supports"
  "and" "not" "of" "only" "or" "selector"
] @keyword
[(from) (to)] @keyword
(keyword_query) @constant.builtin

(comment) @comment
(js_comment) @comment
