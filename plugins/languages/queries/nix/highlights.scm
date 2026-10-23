; Nix: attribute names as properties, builtins as builtins.

(binding attrpath: (attrpath (identifier) @property))
(inherited_attrs (identifier) @property)
(formal name: (identifier) @constant)
(function_expression universal: (identifier) @constant)
(apply_expression function: (variable_expression name: (identifier) @function.call))
(apply_expression function: (select_expression attrpath: (attrpath (identifier) @function.call)))
((apply_expression function: (variable_expression name: (identifier) @function.builtin))
 (#any-of? @function.builtin
  "abort" "derivation" "fetchGit" "fetchTarball" "fetchurl" "import" "isNull" "map"
  "removeAttrs" "throw" "toString"))
((variable_expression name: (identifier) @function.builtin)
 (#eq? @function.builtin "builtins"))
((identifier) @boolean (#any-of? @boolean "true" "false"))
((identifier) @constant.builtin (#eq? @constant.builtin "null"))

[(string_expression) (indented_string_expression)] @string
(interpolation) @embedded
[(escape_sequence) (dollar_escape)] @string.escape
[(path_expression) (hpath_expression) (spath_expression)] @string.special
(uri_expression) @markup.link
[(integer_expression) (float_expression)] @number
(comment) @comment

["assert" "else" "if" "in" "inherit" "let" "or" "rec" "then" "with"] @keyword
