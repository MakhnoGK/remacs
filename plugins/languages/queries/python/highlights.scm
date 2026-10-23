; Python. For one node the last pattern wins; a node inside another draws over it.

(function_definition name: (identifier) @function)
(call function: (identifier) @function.call)
(call function: (attribute attribute: (identifier) @function.call))
((call function: (identifier) @function.builtin)
 (#any-of? @function.builtin
  "abs" "all" "any" "ascii" "bin" "bool" "breakpoint" "bytearray" "bytes" "callable" "chr"
  "classmethod" "compile" "complex" "delattr" "dict" "dir" "divmod" "enumerate" "eval"
  "exec" "filter" "float" "format" "frozenset" "getattr" "globals" "hasattr" "hash" "help"
  "hex" "id" "input" "int" "isinstance" "issubclass" "iter" "len" "list" "locals" "map"
  "max" "memoryview" "min" "next" "object" "oct" "open" "ord" "pow" "print" "property"
  "range" "repr" "reversed" "round" "set" "setattr" "slice" "sorted" "staticmethod" "str"
  "sum" "super" "tuple" "type" "vars" "zip" "__import__"))

; Capitalized names are classes; all-caps ones constants.
((identifier) @type (#match? @type "^[A-Z]"))
((identifier) @constant (#match? @constant "^[A-Z][A-Z0-9_]+$"))
(class_definition name: (identifier) @type)
(type (identifier) @type)
((identifier) @variable.builtin (#any-of? @variable.builtin "self" "cls"))
(decorator) @attribute

(string) @string
(interpolation) @embedded
[(escape_sequence) (escape_interpolation)] @string.escape
[(integer) (float)] @number
[(true) (false)] @boolean
(none) @constant.builtin
(comment) @comment

; Docstrings: a string first in a module, class or function.
(module . (expression_statement (string) @comment.doc))
(class_definition body: (block . (expression_statement (string) @comment.doc)))
(function_definition body: (block . (expression_statement (string) @comment.doc)))

[
  "and" "as" "assert" "async" "await" "break" "case" "class" "continue" "def" "del" "elif"
  "else" "except" "exec" "finally" "for" "from" "global" "if" "import" "in" "is" "lambda"
  "match" "nonlocal" "not" "or" "pass" "print" "raise" "return" "try" "type" "while" "with"
  "yield"
] @keyword
