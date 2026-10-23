; Extends ECMAScript's: an element's children, and a tag's attributes, are a level deeper.
[
  (jsx_element)
  (jsx_opening_element)
  (jsx_self_closing_element)
  (jsx_expression)
] @indent.begin

(jsx_closing_element) @indent.end

(jsx_opening_element ">" @indent.end)

(jsx_self_closing_element "/>" @indent.end)
