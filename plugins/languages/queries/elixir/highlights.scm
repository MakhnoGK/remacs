; Elixir, where `def`, `if` and the like are calls: those are keywords here.

(call target: (identifier) @function.call)
(call target: (dot right: (identifier) @function.call))
((call target: (identifier) @keyword
  (arguments [(identifier) @function (call target: (identifier) @function)]))
 (#any-of? @keyword
  "def" "defp" "defmacro" "defmacrop" "defguard" "defguardp" "defdelegate"))
((call target: (identifier) @keyword)
 (#any-of? @keyword
  "alias" "case" "cond" "def" "defdelegate" "defexception" "defguard" "defguardp"
  "defimpl" "defmacro" "defmacrop" "defmodule" "defoverridable" "defp" "defprotocol"
  "defstruct" "for" "if" "import" "quote" "raise" "receive" "require" "reraise" "super"
  "throw" "try" "unless" "unquote" "use" "with"))

(alias) @type
[(atom) (quoted_atom) (keyword) (quoted_keyword)] @constant
(unary_operator operator: "@" @attribute operand: (_) @attribute)
(unary_operator operator: "@" operand: (call target: (identifier) @attribute))

[(string) (charlist) (char)] @string
(sigil) @string.special
(interpolation) @embedded
(escape_sequence) @string.escape
[(integer) (float)] @number
(boolean) @boolean
(nil) @constant.builtin
(comment) @comment
(unary_operator
  operator: "@"
  operand: (call target: (identifier) @_doc (arguments (string) @comment.doc))
  (#any-of? @_doc "doc" "moduledoc" "typedoc"))

["after" "and" "catch" "do" "else" "end" "fn" "in" "not" "or" "rescue" "when"] @keyword
