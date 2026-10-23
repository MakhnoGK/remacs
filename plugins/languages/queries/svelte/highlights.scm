; Svelte components, as HTML with `{#if}`, `{#each}` and `{@html}` blocks; scripts,
; styles and expressions are their languages (injections.scm).

(tag_name) @tag
(erroneous_end_tag_name) @tag
((tag_name) @type (#match? @type "^[A-Z]"))
(attribute_name) @attribute
(quoted_attribute_value) @string
(block_start_tag) @keyword
(block_end_tag) @keyword
(block_tag) @keyword
(expression_tag) @keyword
(snippet_name) @function
(entity) @constant
(doctype) @preproc
(comment) @comment
