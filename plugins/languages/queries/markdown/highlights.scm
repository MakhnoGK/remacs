; Markdown's blocks: headings, code, quotes and link definitions. Text inside the blocks
; (emphasis, inline code, links) is a grammar of its own.

(atx_heading) @markup.heading
(setext_heading) @markup.heading
[(fenced_code_block) (indented_code_block)] @markup.raw
(info_string (language) @type)
(block_quote) @comment.doc
(link_reference_definition (link_label) @markup.link)
(link_destination) @markup.link
(link_title) @string
[(minus_metadata) (plus_metadata)] @preproc
(html_block) @preproc
[(backslash_escape) (entity_reference) (numeric_character_reference)] @string.escape
