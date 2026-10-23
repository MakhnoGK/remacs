; Markdown inside its blocks: code, links and markup.

(code_span) @markup.raw
[(link_text) (image_description)] @markup.link
[(link_destination) (uri_autolink) (email_autolink)] @markup.link
(link_label) @markup.link
(html_tag) @tag
[(backslash_escape) (entity_reference) (numeric_character_reference)] @string.escape
