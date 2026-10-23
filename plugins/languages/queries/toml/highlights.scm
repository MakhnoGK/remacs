; TOML: tables as types, keys as properties.

(bare_key) @property
(quoted_key) @property
(table [(bare_key) (dotted_key) (quoted_key)] @type)
(table_array_element [(bare_key) (dotted_key) (quoted_key)] @type)
(table (dotted_key (bare_key) @type))
(table_array_element (dotted_key (bare_key) @type))

(string) @string
(escape_sequence) @string.escape
[(integer) (float)] @number
(boolean) @boolean
[(offset_date_time) (local_date_time) (local_date) (local_time)] @number
(comment) @comment
