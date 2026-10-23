; Vue single-file components, as HTML; scripts, styles and template expressions are
; their languages (injections.scm).

(tag_name) @tag
(erroneous_end_tag_name) @tag
((tag_name) @type (#match? @type "^[A-Z]"))
(attribute_name) @attribute
(directive_name) @keyword
(directive_attribute ":" @keyword)
(directive_modifier) @attribute
(directive_value) @attribute
[(attribute_value) (quoted_attribute_value)] @string
(interpolation ["{{" "}}"] @keyword)
(entity) @constant
(doctype) @preproc
(comment) @comment
