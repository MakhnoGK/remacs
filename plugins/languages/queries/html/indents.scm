; An element's content, and a tag's attributes, are a level deeper.
[
  (element)
  (script_element)
  (style_element)
  (start_tag)
  (self_closing_tag)
] @indent.begin

(end_tag) @indent.end

(start_tag ">" @indent.end)

(self_closing_tag "/>" @indent.end)

(start_tag ">" @indent.open)
