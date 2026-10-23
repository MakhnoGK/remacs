; JSX elements, after the ECMAScript patterns: tags as HTML's, components as types.

(jsx_opening_element name: (identifier) @tag)
(jsx_closing_element name: (identifier) @tag)
(jsx_self_closing_element name: (identifier) @tag)
(jsx_opening_element name: (identifier) @type (#match? @type "^[A-Z]"))
(jsx_closing_element name: (identifier) @type (#match? @type "^[A-Z]"))
(jsx_self_closing_element name: (identifier) @type (#match? @type "^[A-Z]"))
(jsx_opening_element name: (member_expression) @type)
(jsx_closing_element name: (member_expression) @type)
(jsx_self_closing_element name: (member_expression) @type)
(jsx_attribute (property_identifier) @attribute)
