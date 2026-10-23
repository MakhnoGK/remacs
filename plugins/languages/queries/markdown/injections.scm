; Each block's text is the inline grammar's, painted over the block; a fenced block is
; the language its info string names.

((inline) @injection.content
 (#set! injection.language "markdown_inline")
 (#set! injection.overlay))

(fenced_code_block
  (info_string (language) @injection.language)
  (code_fence_content) @injection.content)

; The short names READMEs use.
((fenced_code_block
   (info_string (language) @_language)
   (code_fence_content) @injection.content)
 (#any-of? @_language "js" "mjs" "cjs" "node")
 (#set! injection.language "javascript"))
((fenced_code_block
   (info_string (language) @_language)
   (code_fence_content) @injection.content)
 (#any-of? @_language "ts" "mts" "cts")
 (#set! injection.language "typescript"))
((fenced_code_block
   (info_string (language) @_language)
   (code_fence_content) @injection.content)
 (#any-of? @_language "sh" "shell" "zsh" "console")
 (#set! injection.language "bash"))
((fenced_code_block
   (info_string (language) @_language)
   (code_fence_content) @injection.content)
 (#any-of? @_language "py" "python3")
 (#set! injection.language "python"))
((fenced_code_block
   (info_string (language) @_language)
   (code_fence_content) @injection.content)
 (#any-of? @_language "rs")
 (#set! injection.language "rust"))
((fenced_code_block
   (info_string (language) @_language)
   (code_fence_content) @injection.content)
 (#any-of? @_language "rb")
 (#set! injection.language "ruby"))
((fenced_code_block
   (info_string (language) @_language)
   (code_fence_content) @injection.content)
 (#any-of? @_language "yml")
 (#set! injection.language "yaml"))
((fenced_code_block
   (info_string (language) @_language)
   (code_fence_content) @injection.content)
 (#any-of? @_language "md")
 (#set! injection.language "markdown"))
((fenced_code_block
   (info_string (language) @_language)
   (code_fence_content) @injection.content)
 (#any-of? @_language "kt" "kts")
 (#set! injection.language "kotlin"))
((fenced_code_block
   (info_string (language) @_language)
   (code_fence_content) @injection.content)
 (#any-of? @_language "cpp" "cxx" "hpp")
 (#set! injection.language "c++"))
((fenced_code_block
   (info_string (language) @_language)
   (code_fence_content) @injection.content)
 (#any-of? @_language "cs" "csharp")
 (#set! injection.language "c#"))
((fenced_code_block
   (info_string (language) @_language)
   (code_fence_content) @injection.content)
 (#any-of? @_language "tf" "terraform")
 (#set! injection.language "hcl"))
