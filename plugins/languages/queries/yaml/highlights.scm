; YAML: keys as properties, anchors and aliases as labels, tags as types.

[(double_quote_scalar) (single_quote_scalar) (block_scalar)] @string
(escape_sequence) @string.escape
[(integer_scalar) (float_scalar) (timestamp_scalar)] @number
(boolean_scalar) @boolean
(null_scalar) @constant.builtin

(block_mapping_pair key: (flow_node) @property)
(flow_pair key: (flow_node) @property)

(anchor) @label
(alias) @label
(tag) @type
[(yaml_directive) (tag_directive) (reserved_directive)] @preproc
(comment) @comment
