; Haskell. For one node the last pattern wins; a node inside another draws over it.

(function name: (variable) @function)
(signature name: (variable) @function)
(bind name: (variable) @function)
(apply function: (variable) @function.call)

(name) @type
(constructor) @type
(module) @type
(qualified module: (module) @type)
(pragma) @preproc

[(string) (char)] @string
(quasiquote) @string.special
[(integer) (float)] @number
(comment) @comment
((comment) @comment.doc (#match? @comment.doc "^--[ ]*[|^]"))
(haddock) @comment.doc

[
  "anyclass" "as" "case" "class" "data" "default" "deriving" "do" "else" "family" "forall"
  "foreign" "hiding" "if" "import" "in" "infix" "infixl" "infixr" "instance" "let" "mdo"
  "module" "newtype" "of" "pattern" "qualified" "rec" "stock" "then" "type" "via" "where"
] @keyword
