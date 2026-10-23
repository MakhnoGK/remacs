; A macro's arguments are tokens to Rust's grammar, so the code in them, `format!`'s and
; `assert_eq!`'s mostly, is parsed as Rust and painted over them, its strings and comments
; left to the file's tree; it stays the host's for editing.
((macro_invocation (token_tree) @injection.content)
 (#set! injection.language "rust_macro_arguments")
 (#set! injection.overlay))
